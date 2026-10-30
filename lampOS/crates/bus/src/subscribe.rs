//! The receiving side: a datagram socket in the topic's `subs/`, and the
//! count of what did not reach it.
//!
//! Nothing promises a subscriber every message. What it is promised is that
//! it can tell: each message says how many of its publisher's came before it
//! and were not received here, and for a topic whose values replace each
//! other the newest is always in the `latest` file, which a subscriber with
//! nothing waiting looks at.
//!
//! It is also told what no message can say: that the topic's source has
//! gone, that a topic which promised a period is late, and how many messages
//! were passed over because they had expired or were of another type than
//! the one it asked for (`Notice`, `Statistics`).
use crate::{
    Bus, FILE_MODE, MOST_SOCKET_PATH_BYTES,
    declare::{self, Declaration},
    live::{self, Source},
    name,
    wire::{self, Message},
};
use std::{
    collections::VecDeque,
    fs, io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Publishers whose count a subscriber keeps. With more on one topic, the
/// one first heard is forgotten, and its next message counts as its first.
const MOST_PUBLISHERS: usize = 32;

/// A path taken by another socket is passed over this many times.
const TRIES: usize = 8;

/// Notices kept for a caller that has not asked for them: the oldest goes
/// when there are more.
const MOST_NOTICES: usize = 32;

/// What a subscriber learns of its topic that no message says. Each is
/// given once, when it becomes true, by `Subscriber::notices`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notice {
    /// No publisher of the topic has shown itself alive within the lease,
    /// and no message has come for as long. Also said of a topic whose
    /// source never came, a lease after subscribing.
    SourceGone,
    /// After that: a publisher shows itself alive, or a message came.
    SourceBack,
    /// The topic declares a period, and nothing has come for more than
    /// twice as long.
    Late,
    /// After that: a message came.
    OnTime,
}

/// What a subscriber has had from its topic since it was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    /// Messages `recv` handed over.
    pub received: u64,
    /// Messages known to have been sent and not received: the sum of every
    /// message's `gap`.
    pub gaps: u64,
    /// Messages passed over because they had outlived their `expires_ms`.
    pub expired: u64,
    /// Messages passed over because their header named another type than
    /// the one this subscriber expects.
    pub wrong_type: u64,
    /// Of `received`, those that came as datagrams, not from the topic's
    /// `latest` file: none until a publisher has found this subscriber.
    pub datagrams: u64,
    /// The mean time between `recv` taking one message from its socket and
    /// taking the next, by this process's monotonic clock: the stream's own
    /// rhythm for a subscriber that reads as fast as messages come, and the
    /// reader's for one that does not. `None` until two have been taken.
    pub mean_interval: Option<Duration>,
    /// The longest of those.
    pub largest_interval: Option<Duration>,
    /// The mean of this process's wall clock at arrival less the message's
    /// `at_unix_ms`: how old a message is when it gets here.
    pub mean_age: Option<Duration>,
}

/// What `Statistics` is worked out from.
#[derive(Default)]
struct Counts {
    received: u64,
    gaps: u64,
    expired: u64,
    wrong_type: u64,
    datagrams: u64,
    intervals: u64,
    interval_sum: Duration,
    largest_interval: Duration,
    age_sum_ms: u64,
}

/// When a message says it was sent, by the clock it is judged by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sent {
    /// This boot's CLOCK_MONOTONIC, in microseconds: the header said so.
    Monotonic(u64),
    /// The wall clock, in milliseconds since 1970: the header said nothing.
    Wall(u64),
}

impl Sent {
    fn of(header: &wire::Header, now: &wire::Now<'_>) -> Self {
        match header.stamp_clock(now) {
            wire::StampClock::Monotonic => Self::Monotonic(header.at_us),
            _ => Self::Wall(header.at_unix_ms),
        }
    }

    /// Whether more than `limit_ms` passed between `before` and this. Two
    /// stamps of different clocks, as a round of updates that mixes builds
    /// sends, are not measured against each other: the subscriber's own
    /// clock still says a silence it saw (`overdue`).
    fn after_more_than(self, before: Self, limit_ms: u64) -> bool {
        match (before, self) {
            (Self::Monotonic(before), Self::Monotonic(at)) => {
                at.saturating_sub(before) > limit_ms.saturating_mul(1_000)
            }
            (Self::Wall(before), Self::Wall(at)) => at.saturating_sub(before) > limit_ms,
            _ => false,
        }
    }
}

/// The last message had from one publisher.
struct Heard {
    source: String,
    boot: String,
    seq: u64,
    /// The instance heard under this name before the present one.
    before: Option<String>,
    /// Missed before a message that was passed over, and so not yet said
    /// with any message the caller was given.
    owed: u64,
    /// The same for a new instance whose first message was passed over.
    restarted: bool,
}

pub struct Subscriber {
    topic: String,
    path: PathBuf,
    latest: PathBuf,
    socket: UnixDatagram,
    buffer: Vec<u8>,
    /// So many bytes of `buffer` are a datagram read ahead of the caller's
    /// next `recv`, to find out whether anything was waiting.
    held: Option<usize>,
    /// What the socket has been told to wait: nothing yet, or this long.
    waits: Option<Duration>,
    heard: Vec<Heard>,
    looked: Option<Instant>,
    look_every: Duration,
    malformed: u64,
    repeated: u64,
    counts: Counts,
    /// The type this subscriber asked for, if it asked for one.
    expects: Option<String>,
    declaration: PathBuf,
    /// What the topic's declaration said when it was last read.
    declared: Option<Declaration>,
    learned: Option<Instant>,
    pubs: PathBuf,
    lease: Duration,
    began: Instant,
    /// When a datagram of the topic last came, handed over or not.
    spoke: Option<Instant>,
    /// When a datagram that was handed over was last taken, and when its
    /// publisher says it sent it.
    arrived: Option<Instant>,
    sent: Option<Sent>,
    /// Since when the topic's period has been known: no silence before
    /// that was late.
    timed_from: Instant,
    gone: bool,
    late: bool,
    /// When `pubs/` was last looked at, and whether anyone was alive there.
    sought: Option<Instant>,
    found: bool,
    notices: VecDeque<Notice>,
    /// The bus's clocks: what has expired, and the gap between two sends.
    clocks: crate::clock::Clocks,
}

/// The subscriber's socket, for a caller that waits on several at once
/// with `poll` instead of asking each in turn (#705, the lamp's heat). It is
/// readable when a datagram is waiting; one read ahead is not
/// ([`Subscriber::holds`]). The topic's file and its publishers are looked
/// at only in `recv`, so a caller still calls it at least every
/// [`Bus::rescan_every`].
impl std::os::fd::AsFd for Subscriber {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.socket.as_fd()
    }
}

impl Subscriber {
    pub(crate) fn new(
        bus: &Bus,
        topic: &str,
        subscriber: &str,
        expects: Option<&str>,
    ) -> io::Result<Self> {
        static BOUND: AtomicU64 = AtomicU64::new(0);
        name::topic(topic)?;
        name::subscriber(subscriber)?;
        if let Some(expects) = expects {
            name::type_name(expects)?;
        }
        let directory = bus.root.join(topic);
        let subs = directory.join("subs");
        // Before anything is created: a subscriber that is refused leaves
        // nothing behind. A topic nobody has declared yet is not refused: a
        // subscriber may start before its publisher, and every message says
        // its own type. Nor is one whose declaration cannot be read.
        let declaration = directory.join("declaration");
        let declared = declare::read(&declaration).ok().flatten();
        if let (Some(declared), Some(expects)) = (&declared, expects) {
            declare::expected(topic, declared, expects)?;
        }
        let mut made = false;
        for _ in 0..TRIES {
            let stem = format!(
                "{subscriber}.{}.{}",
                process::id(),
                BOUND.fetch_add(1, Ordering::Relaxed)
            );
            let path = subs.join(format!("{stem}.sock"));
            let length = path.as_os_str().len();
            if length > MOST_SOCKET_PATH_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "socket path {} is {length} bytes and a Unix socket's address holds \
                         {MOST_SOCKET_PATH_BYTES}: use a shorter root, topic or subscriber name",
                        path.display()
                    ),
                ));
            }
            if !made {
                crate::make_directory(&subs, bus.directory_mode)?;
                made = true;
            }
            // A socket's file can be seen a moment before the socket can be
            // sent to, and a publisher that looks in that moment finds it
            // refused and removes it as a dead subscriber's: 61 of 400
            // subscribers lost theirs that way to a publisher that looked
            // with every message (macOS, 2026-10-13). So the socket is bound
            // under a name publishers pass over, and is given its own only
            // once it is whole. By a link and not a rename, which would
            // replace whatever had that name.
            let birth = subs.join(format!("{stem}.new"));
            let socket = match UnixDatagram::bind(&birth) {
                Ok(socket) => socket,
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("bind {}: {error}", birth.display()),
                    ));
                }
            };
            let named = fs::set_permissions(&birth, fs::Permissions::from_mode(FILE_MODE))
                .and_then(|()| fs::hard_link(&birth, &path));
            let _ = fs::remove_file(&birth);
            match named {
                Ok(()) => {}
                // Left by a process that had this number before. A
                // publisher will remove it; this one takes the next name.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("name {}: {error}", path.display()),
                    ));
                }
            }
            let now = Instant::now();
            return Ok(Self {
                topic: topic.into(),
                path,
                latest: directory.join("latest"),
                socket,
                buffer: vec![0; wire::MOST_BYTES + 1],
                held: None,
                waits: None,
                heard: Vec::new(),
                looked: None,
                look_every: bus.rescan_every,
                malformed: 0,
                repeated: 0,
                counts: Counts::default(),
                expects: expects.map(str::to_string),
                declaration,
                declared,
                learned: Some(now),
                pubs: directory.join("pubs"),
                lease: bus.lease,
                began: now,
                spoke: None,
                arrived: None,
                sent: None,
                timed_from: now,
                gone: false,
                late: false,
                sought: None,
                found: false,
                notices: VecDeque::new(),
                clocks: bus.clocks.clone(),
            });
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("no free socket name for {subscriber} in {}", subs.display()),
        ))
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Whether a datagram was already read ahead of the next `recv` (to see
    /// whether anything was waiting): it is no longer in the socket, so a
    /// `poll` on [`AsFd`](std::os::fd::AsFd) does not show it. A caller that
    /// waits on the socket asks this first.
    pub fn holds(&self) -> bool {
        self.held.is_some()
    }

    /// Datagrams that arrived and were not a message of this topic.
    pub fn malformed(&self) -> u64 {
        self.malformed
    }

    /// Messages that arrived after a newer one from the same publisher had
    /// already been returned, and were passed over.
    pub fn repeated(&self) -> u64 {
        self.repeated
    }

    /// Messages `recv` passed over because they had outlived the time their
    /// header gives them.
    pub fn expired(&self) -> u64 {
        self.counts.expired
    }

    /// Messages passed over because their header named another type than
    /// the one this subscriber asked for.
    pub fn wrong_type(&self) -> u64 {
        self.counts.wrong_type
    }

    /// The type this subscriber asked for, if it asked for one.
    pub fn expects(&self) -> Option<&str> {
        self.expects.as_deref()
    }

    /// The topic's declaration as this subscriber last read it: when it
    /// subscribed, when a publisher it had not heard before spoke, and,
    /// while there is none, when it is asked for notices.
    pub fn declaration(&self) -> Option<&Declaration> {
        self.declared.as_ref()
    }

    /// What has come from the topic since this subscriber was made.
    pub fn statistics(&self) -> Statistics {
        let counts = &self.counts;
        let mean =
            |sum: Duration, of: u64| Duration::from_nanos((sum.as_nanos() / of as u128) as u64);
        Statistics {
            received: counts.received,
            gaps: counts.gaps,
            expired: counts.expired,
            wrong_type: counts.wrong_type,
            datagrams: counts.datagrams,
            mean_interval: (counts.intervals > 0)
                .then(|| mean(counts.interval_sum, counts.intervals)),
            largest_interval: (counts.intervals > 0).then_some(counts.largest_interval),
            mean_age: (counts.received > 0)
                .then(|| mean(Duration::from_millis(counts.age_sum_ms), counts.received)),
        }
    }

    /// The publishers of the topic that have shown themselves alive within
    /// the lease, as `pubs/` has them now. Looking removes the entry of one
    /// that has not.
    pub fn sources(&self) -> io::Result<Vec<Source>> {
        live::list(&self.pubs, self.lease)
    }

    /// What this subscriber's clocks say now: to judge a header by as
    /// `recv` judges it ([`wire::Header::expired_by`]).
    pub fn now(&self) -> wire::Now<'_> {
        self.clocks.now()
    }

    /// What has changed about the topic since this was last asked: its
    /// source gone or back, the topic late or on time again. Each change is
    /// given once, in the order it was noticed.
    ///
    /// `recv` returns messages and nothing else, as it always did, so a
    /// caller that wants these asks between two `recv`s; one that never
    /// asks pays for none of it but a count. The state is judged when this
    /// is called and when a message is taken, so a notice is as late as the
    /// caller's own loop. `pubs/` is looked at only for a topic that has
    /// been silent for a lease, at most once in the time a publisher takes
    /// to notice a new subscriber.
    ///
    /// A caller that has not read for a while is not told that its topic
    /// went quiet: silence is only said when nothing is waiting in the
    /// socket either, and a silence found afterwards is measured between
    /// the times two messages were sent, not the times they were read.
    pub fn notices(&mut self) -> Vec<Notice> {
        self.judge(Instant::now());
        self.notices.drain(..).collect()
    }

    /// Whether the topic declares a period and nothing has come for more
    /// than twice as long. False for a topic that promises no period.
    pub fn is_late(&mut self) -> bool {
        self.judge(Instant::now());
        self.late
    }

    /// Whether the topic has no source: no publisher alive within the
    /// lease, and no message for as long.
    pub fn source_gone(&mut self) -> bool {
        self.judge(Instant::now());
        self.gone
    }

    fn notice(&mut self, notice: Notice) {
        if self.notices.len() == MOST_NOTICES {
            self.notices.pop_front();
        }
        self.notices.push_back(notice);
    }

    /// Read the topic's declaration again. One that cannot be read just now
    /// is no reason to forget the one known.
    fn learn(&mut self, now: Instant) {
        self.learned = Some(now);
        let Ok(Some(found)) = declare::read(&self.declaration) else {
            return;
        };
        let timed = self
            .declared
            .as_ref()
            .is_some_and(|known| known.period_ms.is_some());
        if !timed && found.period_ms.is_some() {
            self.timed_from = now;
        }
        self.declared = Some(found);
    }

    /// Since when the topic has been silent, for a topic with a period.
    fn silent_since(&self) -> Instant {
        self.arrived
            .map_or(self.timed_from, |at| at.max(self.timed_from))
    }

    fn overdue(&self, now: Instant) -> bool {
        self.declared
            .as_ref()
            .and_then(Declaration::period)
            .is_some_and(|period| now.saturating_duration_since(self.silent_since()) > 2 * period)
    }

    /// Whether a datagram is waiting to be received. One that is, is read
    /// ahead and kept for the next `recv`: the standard library has no way
    /// to ask without taking it.
    fn waiting(&mut self) -> bool {
        if self.held.is_some() {
            return true;
        }
        if self.wait(Duration::ZERO).is_err() {
            return false;
        }
        loop {
            match self.socket.recv(&mut self.buffer) {
                Ok(length) => {
                    self.held = Some(length);
                    return true;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
        }
    }

    /// Whether any publisher shows itself alive, as of the last look.
    fn any_source(&mut self, now: Instant) -> bool {
        if self
            .sought
            .is_none_or(|at| now.saturating_duration_since(at) >= self.look_every)
        {
            self.sought = Some(now);
            self.found = live::list(&self.pubs, self.lease).is_ok_and(|found| !found.is_empty());
        }
        self.found
    }

    /// Bring what is known of the topic up to `now`, and note what changed.
    fn judge(&mut self, now: Instant) {
        if self.declared.is_none()
            && self
                .learned
                .is_none_or(|at| now.saturating_duration_since(at) >= self.look_every)
        {
            self.learn(now);
        }
        // What has not been taken for a while may be waiting to be: then
        // it is this subscriber that is behind, and not its topic.
        if !self.late && self.overdue(now) && !self.waiting() {
            self.late = true;
            self.notice(Notice::Late);
        }
        if self.gone {
            // A publisher that has started and not yet spoken is a source.
            if self.any_source(now) {
                self.gone = false;
                self.notice(Notice::SourceBack);
            }
            return;
        }
        // A message within the lease is a source, whatever `pubs/` says, so
        // a topic that streams is never looked up there.
        let since = self.spoke.map_or(self.began, |at| at.max(self.began));
        if now.saturating_duration_since(since) > self.lease
            && !self.any_source(now)
            && !self.waiting()
        {
            self.gone = true;
            self.notice(Notice::SourceGone);
        }
    }

    /// A datagram of the topic came: somebody publishes it.
    fn heard_from(&mut self, now: Instant) {
        self.spoke = Some(now);
        if self.gone {
            self.gone = false;
            self.notice(Notice::SourceBack);
        }
    }

    /// Whether the header names another type than the one asked for. A
    /// message that names none is taken: it comes from a publisher that
    /// declared nothing, or from a recording made before there were types.
    fn other_type(&self, header: &wire::Header) -> bool {
        match (&self.expects, &header.type_name) {
            (Some(expects), Some(carried)) => expects != carried,
            _ => false,
        }
    }

    /// Decide whether a message that is news is handed to the caller, and
    /// count it either way. `sent` says that it came as a datagram and not
    /// from the topic's `latest` file: only that shows a publisher alive
    /// now, and only that has a time of arrival.
    fn admit(&mut self, mut message: Message, sent: bool) -> Option<Message> {
        let now = Instant::now();
        // By the clock the header calls for: a this-boot monotonic stamp
        // by `at_us`, which a time server's step does not move, and only a
        // header that says no clock by the wall's.
        let (wall, expired, stamp) = {
            let clocks = self.clocks.now();
            (
                clocks.unix_ms,
                message.header.expired_by(&clocks),
                Sent::of(&message.header, &clocks),
            )
        };
        if sent {
            self.heard_from(now);
        }
        let other = self.other_type(&message.header);
        let heard = self
            .heard
            .iter_mut()
            .find(|heard| heard.source == message.header.source);
        if other || expired {
            if other {
                self.counts.wrong_type += 1;
            } else {
                self.counts.expired += 1;
            }
            // What this message would have said of its publisher is said
            // with the next one the caller is given.
            if let Some(heard) = heard {
                heard.owed += message.gap;
                heard.restarted |= message.publisher_restarted;
            }
            return None;
        }
        if let Some(heard) = heard {
            message.gap += std::mem::take(&mut heard.owed);
            message.publisher_restarted |= std::mem::take(&mut heard.restarted);
        }
        self.counts.received += 1;
        let age = wall.saturating_sub(message.header.at_unix_ms);
        self.counts.age_sum_ms = self.counts.age_sum_ms.saturating_add(age);
        if sent {
            self.came(now, stamp);
        }
        Some(message)
    }

    /// A datagram that is handed over was taken at `now` and sent at
    /// `sent`: its distance from the one before, and the end of a silence
    /// that was too long.
    fn came(&mut self, now: Instant, sent: Sent) {
        self.counts.datagrams += 1;
        if let Some(before) = self.arrived {
            let interval = now.saturating_duration_since(before);
            self.counts.intervals += 1;
            self.counts.interval_sum += interval;
            self.counts.largest_interval = self.counts.largest_interval.max(interval);
        }
        // A silence nobody asked about while it lasted is said now, with
        // its end, so that each is said once whenever the caller looks. It
        // is measured between the times the two messages were sent: when
        // this one was read says how busy the reader was, not how quiet
        // the topic. By `at_us` when both say they are this boot's monotonic
        // clock: a wall clock stepped forward between them is no silence.
        let period = self.declared.as_ref().and_then(|known| known.period_ms);
        let hole = self
            .sent
            .zip(period)
            .is_some_and(|(before, period)| sent.after_more_than(before, period.saturating_mul(2)));
        if !self.late && hole {
            self.late = true;
            self.notice(Notice::Late);
        }
        if self.late {
            self.late = false;
            self.notice(Notice::OnTime);
        }
        self.arrived = Some(now);
        self.sent = Some(sent);
    }

    /// The next message, waiting at most `timeout` for one. What is not a
    /// message of this topic is counted and passed over, and so is a message
    /// that has outlived the `expires_ms` in its header by more than 50 ms
    /// (`expired`), and one whose header names another type than this
    /// subscriber asked for (`wrong_type`). The time is the clock the header
    /// calls for ([`wire::Header::expired_by`]): this boot's monotonic clock
    /// for a header whose `clock` is this boot, and then also a stamp more
    /// than 50 ms ahead is passed over as expired; none for one of another
    /// boot, which has always expired; and the wall clock for one that says
    /// no clock.
    ///
    /// With nothing waiting, a topic's `latest` file is looked at, at most
    /// once in the time a publisher takes to notice a new subscriber, and
    /// returned if it is newer than what this subscriber has had. That is
    /// what makes the last value of a topic safe: the message that fell on a
    /// full queue, or was sent in the moment before the publisher knew of
    /// this subscriber, has no later one behind it to show the gap.
    pub fn recv(&mut self, timeout: Duration) -> io::Result<Option<Message>> {
        if let Some(message) = self.queued(timeout)? {
            return Ok(Some(message));
        }
        match self.kept_news() {
            Some(kept) => self.queued_or(kept),
            None => Ok(None),
        }
    }

    /// A publisher sends a value to the sockets before it keeps it, so by
    /// the time the file holds one, whatever was delivered of it and of
    /// those before it is in the queue. Those come first, in their order:
    /// the file is ahead only of a queue that is empty, and then nothing
    /// that was delivered is passed over for it.
    fn queued_or(
        &mut self,
        (header, payload): (wire::Header, Vec<u8>),
    ) -> io::Result<Option<Message>> {
        if let Some(message) = self.queued(Duration::ZERO)? {
            return Ok(Some(message));
        }
        // The queue may have held this very value and passed it over as
        // expired: it is then not news a second time.
        if !self.kept_is_news(&header) {
            return Ok(None);
        }
        let message = self.account(header, payload);
        Ok(self.admit(message, false))
    }

    /// The next message from the socket, waiting at most `timeout` for it.
    fn queued(&mut self, timeout: Duration) -> io::Result<Option<Message>> {
        let started = Instant::now();
        let mut left = timeout;
        loop {
            // What was read ahead to see whether anything was waiting
            // comes first.
            let read = match self.held.take() {
                Some(length) => Ok(length),
                None => {
                    self.wait(left)?;
                    self.socket.recv(&mut self.buffer)
                }
            };
            match read {
                Ok(length) => {
                    let arrived = wire::split(&self.topic, &self.buffer[..length])
                        .map(|(header, payload)| (header, payload.to_vec()));
                    match arrived {
                        Ok((header, payload)) if self.news(&header) => {
                            let message = self.account(header, payload);
                            if let Some(message) = self.admit(message, true) {
                                return Ok(Some(message));
                            }
                        }
                        Ok(_) => {
                            self.repeated += 1;
                            self.heard_from(Instant::now());
                        }
                        Err(_) => self.malformed += 1,
                    }
                    // What was passed over does not make the wait longer
                    // than was asked for: once the time is up, only what is
                    // already here is read.
                    left = timeout.saturating_sub(started.elapsed());
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    left = timeout.saturating_sub(started.elapsed());
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// The topic's current value, if it keeps one. It counts as received:
    /// `recv` does not return the same message again.
    ///
    /// A value that has outlived its `expires_ms` is still returned, with
    /// `expired` set: it is the last thing known of the topic, and whether
    /// that is worth anything is the caller's to say. A value of another
    /// type than this subscriber asked for is not returned.
    pub fn latest(&mut self) -> io::Result<Option<Message>> {
        self.looked = Some(Instant::now());
        let Some((header, payload)) = wire::kept(&self.latest, &self.topic)? else {
            return Ok(None);
        };
        let other = self.other_type(&header);
        let expired = header.expired_by(&self.clocks.now());
        if !self.kept_is_news(&header) {
            return Ok((!other).then_some(Message {
                header,
                payload,
                gap: 0,
                publisher_restarted: false,
                expired,
            }));
        }
        let mut message = self.account(header, payload);
        let heard = self
            .heard
            .iter_mut()
            .find(|heard| heard.source == message.header.source);
        if other {
            self.counts.wrong_type += 1;
            if let Some(heard) = heard {
                heard.owed += message.gap;
                heard.restarted |= message.publisher_restarted;
            }
            return Ok(None);
        }
        if let Some(heard) = heard {
            message.gap += std::mem::take(&mut heard.owed);
            message.publisher_restarted |= std::mem::take(&mut heard.restarted);
        }
        message.expired = expired;
        Ok(Some(message))
    }

    /// A read timeout of zero means "for ever" to a socket, and a short one
    /// is rounded up to the kernel's tick, so not waiting at all is asked
    /// for differently. Nothing is asked again while the wait stays the same.
    fn wait(&mut self, left: Duration) -> io::Result<()> {
        if self.waits == Some(left) {
            return Ok(());
        }
        if left.is_zero() {
            self.socket.set_nonblocking(true)?;
        } else {
            if self.waits.is_some_and(|waited| waited.is_zero()) {
                self.socket.set_nonblocking(false)?;
            }
            self.socket.set_read_timeout(Some(left))?;
        }
        self.waits = Some(left);
        Ok(())
    }

    /// What the topic keeps, if this subscriber has not had it and has not
    /// looked a moment ago.
    fn kept_news(&mut self) -> Option<(wire::Header, Vec<u8>)> {
        let now = Instant::now();
        if self
            .looked
            .is_some_and(|at| now.saturating_duration_since(at) < self.look_every)
        {
            return None;
        }
        self.looked = Some(now);
        // A file that cannot be read is no reason to fail a wait: `latest`
        // says what is wrong with it when asked.
        wire::kept(&self.latest, &self.topic)
            .ok()?
            .filter(|(header, _)| self.kept_is_news(header))
    }

    /// Whether the kept value is news. The file is replaced a moment after
    /// the datagrams are sent, so just after a publisher began again it can
    /// still hold the last value of the instance before, which this
    /// subscriber has already heard the successor of.
    fn kept_is_news(&self, header: &wire::Header) -> bool {
        self.news(header)
            && !self.heard.iter().any(|heard| {
                heard.source == header.source && heard.before.as_deref() == Some(&header.boot)
            })
    }

    /// Whether this is later than everything had from its publisher.
    fn news(&self, header: &wire::Header) -> bool {
        self.heard
            .iter()
            .find(|heard| heard.source == header.source)
            .is_none_or(|heard| heard.boot != header.boot || header.seq > heard.seq)
    }

    /// Note a message that is news, and say how it stands to the last one
    /// from its publisher.
    fn account(&mut self, header: wire::Header, payload: Vec<u8>) -> Message {
        let known = self
            .heard
            .iter_mut()
            .find(|heard| heard.source == header.source);
        // The last says whether this publisher instance is one not heard
        // before.
        let (gap, publisher_restarted, new) = match known {
            Some(heard) if heard.boot == header.boot => {
                let gap = header.seq.saturating_sub(heard.seq + 1);
                heard.seq = header.seq;
                (gap, false, false)
            }
            // Another instance under the same name: its count began again,
            // so nothing can be said of what was missed.
            Some(heard) => {
                let before = std::mem::replace(&mut heard.boot, header.boot.clone());
                heard.before = Some(before);
                heard.seq = header.seq;
                heard.owed = 0;
                (0, true, true)
            }
            None => {
                if self.heard.len() == MOST_PUBLISHERS {
                    self.heard.remove(0);
                }
                self.heard.push(Heard {
                    source: header.source.clone(),
                    boot: header.boot.clone(),
                    seq: header.seq,
                    before: None,
                    owed: 0,
                    restarted: false,
                });
                (0, false, true)
            }
        };
        self.counts.gaps += gap;
        // It may have declared the topic anew: the period and the lifespan
        // are the newest publisher's to say.
        if new {
            self.learn(Instant::now());
        }
        Message {
            header,
            payload,
            gap,
            publisher_restarted,
            expired: false,
        }
    }
}

impl Drop for Subscriber {
    /// Publishers stop sending when the file has gone. A subscriber that
    /// dies without getting here leaves it, and the next publish that finds
    /// nobody behind it removes it.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    #[test]
    fn the_kept_value_is_not_taken_ahead_of_messages_still_in_the_queue() {
        let root = std::env::temp_dir().join(format!("lbs{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap().rescan_every(Duration::ZERO);
        let mut subscriber = bus.subscribe("servos", "follow").unwrap();
        let mut publisher = bus.publisher("servos", Kind::Latest, "body").unwrap();
        for yaw in 1..=3u64 {
            assert_eq!(publisher.publish_json(yaw, &yaw).unwrap().delivered, 1);
        }
        // A wait that ended a moment before the three arrived finds the
        // third in the file, and all three in the queue behind it.
        let kept = subscriber.kept_news().expect("the third, in the file");
        assert_eq!(kept.0.seq, 3);
        let message = subscriber.queued_or(kept).unwrap().expect("the first");
        assert_eq!((message.header.seq, message.gap), (1, 0));
        for seq in 2..=3 {
            let message = subscriber.recv(Duration::from_secs(5)).unwrap().unwrap();
            assert_eq!((message.header.seq, message.gap), (seq, 0));
        }
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
        assert_eq!(subscriber.repeated(), 0);
        // All three came as datagrams.
        assert_eq!(subscriber.statistics().datagrams, 3);

        // With the queue empty the file is all there is. This publisher
        // looks once, before the other subscriber is there, and its second
        // value is sent to the first subscriber only.
        drop(publisher);
        let seldom = bus.clone().rescan_every(Duration::from_secs(3_600));
        let mut publisher = seldom.publisher("servos", Kind::Latest, "eyes").unwrap();
        assert_eq!(publisher.publish_json(4, &4).unwrap().delivered, 1);
        let mut other = bus.subscribe("servos", "other").unwrap();
        assert_eq!(publisher.publish_json(5, &5).unwrap().delivered, 1);
        let kept = other.kept_news().expect("the value, in the file");
        let message = other.queued_or(kept).unwrap().expect("the kept value");
        assert_eq!((message.header.seq, message.gap), (2, 0));
        assert_eq!(message.parse::<u64>().unwrap(), 5);
        assert!(other.recv(Duration::ZERO).unwrap().is_none());
        // A value read from the file is received, and is no datagram: the
        // privacy fence resyncs until its first (#1569 row 45).
        let read = other.statistics();
        assert_eq!((read.received, read.datagrams), (1, 0));
        drop((subscriber, other));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_last_value_of_an_instance_that_began_again_is_not_news() {
        let root = std::env::temp_dir().join(format!("lbr{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap().rescan_every(Duration::ZERO);
        let wait = Duration::from_secs(5);
        let mut subscriber = bus.subscribe("servos", "follow").unwrap();
        let mut first = bus.publisher("servos", Kind::Latest, "body").unwrap();
        for yaw in 1..=3u64 {
            first.publish_json(yaw, &yaw).unwrap();
            assert_eq!(subscriber.recv(wait).unwrap().unwrap().header.seq, yaw);
        }
        let kept = root.join("servos/latest");
        let old = fs::read(&kept).unwrap();
        drop(first);
        let mut second = bus.publisher("servos", Kind::Latest, "body").unwrap();
        second.publish_json(9, &9).unwrap();
        let message = subscriber.recv(wait).unwrap().unwrap();
        assert_eq!((message.header.seq, message.publisher_restarted), (1, true));
        // As the file is in the moment between the new instance's first
        // datagram and its first value kept: still the old instance's.
        fs::write(&kept, &old).unwrap();
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
        // Asked for, the file is what it is, and it changes nothing of what
        // has been heard.
        let asked = subscriber.latest().unwrap().unwrap();
        assert_eq!((asked.header.seq, asked.publisher_restarted), (3, false));
        assert_eq!(asked.parse::<u64>().unwrap(), 3);
        second.publish_json(10, &10).unwrap();
        let message = subscriber.recv(wait).unwrap().unwrap();
        assert_eq!((message.header.seq, message.gap), (2, 0));
        assert!(!message.publisher_restarted);
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
        assert_eq!(subscriber.repeated(), 0);
        drop(subscriber);
        fs::remove_dir_all(&root).unwrap();
    }
}
