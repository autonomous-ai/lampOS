//! The loop: it listens to every part's report on the bus, reads the
//! board, and publishes how the lamp is.
//!
//! One thread and no other. Each turn it takes the reports that are
//! waiting, and when one came or a second has passed it works the lamp's
//! health out again. The health is published when it says something new
//! and every five seconds besides; the board is read and published every
//! five seconds; the board's own report goes out once a second, like any
//! part's, and is kept here as well, so that the board is in the lamp's
//! health even if that message is lost.
//!
//! Nothing that goes wrong around it stops it. A topic that cannot be
//! published, a board that cannot be read and messages that are no reports
//! are each said once when they begin and once when they end ([`Note`]),
//! and the loop goes on.
use crate::{
    board::{self, Reading},
    parts::{self, Parts, Taken},
};
use lamp_bus::{Bus, Declaration, Publisher, Subscriber};
use lamp_msgs::{BoardState, Diagnostic, Health, Level, Online, SelfTest};
use serde::Serialize;
use std::{
    fmt, io,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

/// The name this program subscribes and publishes under.
pub const NAME: &str = "health";
/// After how long without a report a part is stale, unless told another.
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(5);
/// No more messages than this are taken in one turn of the loop, so that
/// a flood of reports cannot keep the rest of the turn from happening.
const MOST_AT_ONCE: usize = 64;

/// How often each thing is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Times {
    /// The longest one turn of the loop waits for a report.
    pub tick: Duration,
    /// How often the board's own report is published, and the health
    /// worked out though nothing came.
    pub report_every: Duration,
    /// The lamp's health is published at least this often.
    pub health_every: Duration,
    /// How often the board is read and its numbers published.
    pub board_every: Duration,
}

impl Times {
    /// The times the lamp runs with: a report from the board every second
    /// and the two kept values every five, as their topics promise. With a
    /// stale time under three seconds the board reports three times within
    /// it, so that it is never stale by its own doing.
    pub fn with_stale_after(stale_after: Duration) -> Self {
        Self {
            tick: Duration::from_millis(100),
            report_every: Duration::from_secs(1).min(stale_after / 3),
            health_every: Duration::from_secs(5),
            board_every: Duration::from_secs(5),
        }
    }
}

/// How the loop is to run.
#[derive(Clone, Debug)]
pub struct Options {
    /// The parts that should be reporting.
    pub expected: Vec<String>,
    /// After how long without a report a part is stale.
    pub stale_after: Duration,
    pub times: Times,
    /// Also publish `lamp/online`, once each report's time (#830).
    pub online: bool,
    /// While the voice's link is down, ask the internet itself, at these
    /// targets ([`crate::reach`], `--reach-probe on`): a relay down with the
    /// internet up is then online. Only with `online`.
    pub reach: Option<Vec<std::net::SocketAddr>>,
    /// The probe asks with the link up too, every 30 s, and every 5 s once
    /// the internet doesn't answer; not answering is offline then
    /// ([`crate::reach::Reach::idle`], `--reach-idle on`, #1844). Only with
    /// `reach`.
    pub reach_idle: bool,
    /// Parts that run only where a file is (`--needs`): while it is not
    /// there, the part is listed as off, never missing (#811: tasks without
    /// Harness).
    pub needs: Vec<(String, std::path::PathBuf)>,
}

/// What was counted from start to stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Reports taken from the bus, the board's own among them.
    pub reports: u64,
    /// Messages on `diagnostics` that were no report.
    pub unreadable: u64,
    /// Reports from a part not heard before, when 64 were kept already.
    pub turned_away: u64,
    /// Times the lamp's health and the board's numbers were published.
    pub healths: u64,
    pub boards: u64,
    /// Messages of any of the three topics that could not be published.
    pub unpublished: u64,
}

/// One line the loop says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// The lamp's level is another than it was, or is known for the first
    /// time. The message is the lamp's own, empty when it is ok.
    Health {
        from: Option<Level>,
        to: Level,
        message: String,
    },
    /// A topic cannot be published.
    NotPublishing {
        topic: &'static str,
        kind: io::ErrorKind,
    },
    /// It can again, after this many messages that went nowhere.
    Publishing {
        topic: &'static str,
        unpublished: u64,
    },
    /// Numbers of the board that the lamp cannot do without could not be
    /// read: each with its file and what was the matter with it.
    BoardUnread {
        what: String,
    },
    BoardRead,
    /// A message on `diagnostics` was no report.
    Unreadable,
    /// None such has come for the stale time, after this many.
    Readable {
        passed_over: u64,
    },
    /// The socket for reports could not be read.
    NotReceiving {
        kind: io::ErrorKind,
    },
    Receiving,
    /// A report came from a part not heard before, and 64 are kept.
    TooManyParts,
    /// The self-test's runs cannot be heard: the lamp's health goes on
    /// without its item (#812).
    NoSelfTest {
        kind: io::ErrorKind,
    },
    /// A message on `lamp/selftest` was no run, or not a whole one.
    SelfTestUnreadable,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Health { from, to, message } => {
                f.write_str("health ")?;
                if let Some(from) = from {
                    write!(f, "{} -> ", from.as_str())?;
                }
                f.write_str(to.as_str())?;
                if !message.is_empty() {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Self::NotPublishing { topic, kind } => write!(
                f,
                "{topic} cannot be published ({kind}): it is tried again each second"
            ),
            Self::Publishing { topic, unpublished } => {
                write!(f, "{topic} is published again; unpublished={unpublished}")
            }
            Self::BoardUnread { what } => write!(f, "the board cannot be read: {what}"),
            Self::BoardRead => f.write_str("the board is read again"),
            Self::Unreadable => write!(
                f,
                "a message on {} is not a report: such messages are passed over",
                Diagnostic::TOPIC
            ),
            Self::Readable { passed_over } => write!(
                f,
                "{} carries nothing but reports again; passed_over={passed_over}",
                Diagnostic::TOPIC
            ),
            Self::NotReceiving { kind } => {
                write!(f, "{} cannot be received from ({kind})", Diagnostic::TOPIC)
            }
            Self::Receiving => write!(f, "{} is received from again", Diagnostic::TOPIC),
            Self::TooManyParts => write!(
                f,
                "more than {} parts report: the newcomers are not kept",
                parts::MOST_PARTS
            ),
            Self::NoSelfTest { kind } => write!(
                f,
                "{} cannot be received from ({kind}): the health goes on without it",
                SelfTest::TOPIC
            ),
            Self::SelfTestUnreadable => write!(
                f,
                "a message on {} is not a whole run: it is passed over",
                SelfTest::TOPIC
            ),
        }
    }
}

/// One kind of trouble: counted while it lasts, and known to have begun
/// and to have ended, so that each can be said once.
#[derive(Default)]
struct Trouble {
    on: bool,
    lost: u64,
}

impl Trouble {
    /// It went wrong, and `lost` things with it. True where it begins.
    fn wrong(&mut self, lost: u64) -> bool {
        self.lost += lost;
        !std::mem::replace(&mut self.on, true)
    }

    /// It went right. What was lost while it lasted, where it ends.
    fn right(&mut self) -> Option<u64> {
        std::mem::take(&mut self.on).then(|| std::mem::take(&mut self.lost))
    }
}

/// One topic this program publishes.
///
/// A publisher that fails is let go and made again within a second: one
/// that is kept has never failed, so a message it takes has gone out, and
/// the trouble is over with the first that does.
struct Out {
    topic: &'static str,
    declaration: Declaration,
    publisher: Option<Publisher>,
    trouble: Trouble,
}

impl Out {
    fn new(topic: &'static str, declaration: Declaration) -> Self {
        Self {
            topic,
            declaration,
            publisher: None,
            trouble: Trouble::default(),
        }
    }

    fn failed(&mut self, lost: u64, error: &io::Error, say: &mut dyn FnMut(Note)) {
        self.publisher = None;
        if self.trouble.wrong(lost) {
            say(Note::NotPublishing {
                topic: self.topic,
                kind: error.kind(),
            });
        }
    }

    /// Make the publisher if there is none. Making it declares the topic
    /// and lists this program as its source, before anything is said on it.
    fn make(&mut self, bus: &Bus, say: &mut dyn FnMut(Note)) {
        if self.publisher.is_some() {
            return;
        }
        match bus.publisher_declared(self.topic, &self.declaration, NAME) {
            Ok(publisher) => self.publisher = Some(publisher),
            Err(error) => self.failed(0, &error, say),
        }
    }

    /// Publish `value`. False if it went nowhere.
    fn publish<T: Serialize>(&mut self, at_us: u64, value: &T, say: &mut dyn FnMut(Note)) -> bool {
        let sent = match self.publisher.as_mut() {
            Some(publisher) => publisher.publish_json(at_us, value).map(|_| ()),
            None => Err(io::ErrorKind::NotConnected.into()),
        };
        match sent {
            Ok(()) => {
                if let Some(unpublished) = self.trouble.right() {
                    say(Note::Publishing {
                        topic: self.topic,
                        unpublished,
                    });
                }
                true
            }
            Err(error) => {
                self.failed(1, &error, say);
                false
            }
        }
    }

    /// Show the bus that the publisher is there though it has nothing to
    /// say: the two kept values come five seconds apart, and a publisher
    /// silent for the bus's lease of three is taken for gone.
    fn alive(&mut self, say: &mut dyn FnMut(Note)) {
        if let Some(Err(error)) = self.publisher.as_mut().map(Publisher::alive) {
            self.failed(0, &error, say);
        }
    }
}

/// Everything the loop holds between two turns.
struct Serve<'a> {
    options: &'a Options,
    say: &'a mut dyn FnMut(Note),
    started: Instant,
    parts: Parts,
    /// The board's own report, from the newest reading.
    board_report: Option<Diagnostic>,
    /// The numbers that could not be read at the last reading, as said.
    unread: String,
    /// The lamp's level as last worked out.
    level: Option<Level>,
    /// The health last published.
    published: Option<Health>,
    /// When the health is next published though it is no news: its
    /// period after it last went out, or a report's time after it last
    /// could not.
    health_due: Instant,
    health_out: Out,
    board_out: Out,
    reports_out: Out,
    online_out: Out,
    reach: Option<crate::reach::Reach>,
    unreadable: Trouble,
    /// When a message that was no report last came.
    unreadable_at: Option<Instant>,
    receiving: Trouble,
    /// What the subscriber itself had passed over when last asked.
    passed_over: u64,
    full: bool,
    totals: Totals,
    /// The self-test's runs as they came (#812).
    selftest: parts::SelfTests,
}

impl Serve<'_> {
    fn at_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    /// Read the board: publish its numbers if they are all there, and
    /// keep its report.
    fn read_board(&mut self, read: &mut dyn FnMut() -> Reading) {
        let reading = read();
        let unread = reading
            .unread
            .iter()
            .map(|unread| format!("{} ({})", unread.what, unread.why))
            .collect::<Vec<_>>()
            .join("; ");
        if unread != self.unread {
            (self.say)(if unread.is_empty() {
                Note::BoardRead
            } else {
                Note::BoardUnread {
                    what: unread.clone(),
                }
            });
            self.unread = unread;
        }
        if let Some(state) = reading.state() {
            let at_us = self.at_us();
            if self.board_out.publish(at_us, &state, self.say) {
                self.totals.boards += 1;
            } else {
                self.totals.unpublished += 1;
            }
        }
        self.board_report = Some(reading.report());
    }

    /// Publish the board's report as any part publishes its own, and keep
    /// it here whether or not that message arrives.
    fn report_board(&mut self, now: Instant) {
        let Some(report) = self.board_report.as_ref() else {
            return;
        };
        let at_us = self.started.elapsed().as_micros() as u64;
        if !self.reports_out.publish(at_us, report, self.say) {
            self.totals.unpublished += 1;
        }
        self.parts.take(report, now);
    }

    /// Whether the lamp is online, from the parts' links, when asked for.
    fn report_online(&mut self, now: Instant) {
        if !self.options.online {
            return;
        }
        let mut online = parts::online(self.parts.reports(), now, self.options.stale_after);
        if let Some(reach) = self.reach.as_mut() {
            reach.tend(online.online == Some(false), now);
            online = if self.options.reach_idle {
                parts::with_idle_reach(online, reach.verdict())
            } else {
                parts::with_reach(online, reach.verdict())
            };
        }
        let at_us = self.at_us();
        if !self.online_out.publish(at_us, &online, self.say) {
            self.totals.unpublished += 1;
        }
    }

    fn not_a_report(&mut self, messages: u64) {
        self.totals.unreadable += messages;
        self.unreadable_at = Some(Instant::now());
        if self.unreadable.wrong(messages) {
            (self.say)(Note::Unreadable);
        }
    }

    /// That trouble is over once no such message has come for the stale
    /// time: whoever sent them has gone quiet, as a part does. A report
    /// from another part does not end it. Many parts share the topic, and
    /// one that sends something else every second, among others that
    /// report well, would otherwise be said to begin and end every second.
    fn only_reports(&mut self, now: Instant) {
        if self.unreadable.on
            && self
                .unreadable_at
                .is_none_or(|at| now.saturating_duration_since(at) >= self.options.stale_after)
            && let Some(passed_over) = self.unreadable.right()
        {
            (self.say)(Note::Readable { passed_over });
        }
    }

    /// Count what the subscriber passed over by itself since it was last
    /// asked: a message that named another type, or a datagram that was
    /// no message at all.
    fn passed_over_by(&mut self, ear: &Subscriber) {
        let passed_over = ear.wrong_type() + ear.malformed();
        let more = passed_over - std::mem::replace(&mut self.passed_over, passed_over);
        if more > 0 {
            self.not_a_report(more);
        }
    }

    /// Take the reports that are waiting, after waiting at most `wait`
    /// for the first. True if one was kept.
    fn listen(&mut self, ear: &mut Subscriber, mut wait: Duration) -> bool {
        let mut kept = false;
        for _ in 0..MOST_AT_ONCE {
            let received = ear.recv(wait);
            // What it passed over came before what it now hands over.
            self.passed_over_by(ear);
            let message = match received {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(error) => {
                    if self.receiving.wrong(1) {
                        (self.say)(Note::NotReceiving { kind: error.kind() });
                    }
                    // A socket that fails at once must not spin the loop.
                    thread::sleep(wait);
                    break;
                }
            };
            wait = Duration::ZERO;
            if self.receiving.right().is_some() {
                (self.say)(Note::Receiving);
            }
            let Ok(report) = message.parse::<Diagnostic>() else {
                self.not_a_report(1);
                continue;
            };
            match self.parts.take(&report, Instant::now()) {
                Taken::Kept => {
                    self.totals.reports += 1;
                    kept = true;
                }
                Taken::NotAName => self.not_a_report(1),
                Taken::Full => {
                    self.totals.turned_away += 1;
                    if !std::mem::replace(&mut self.full, true) {
                        (self.say)(Note::TooManyParts);
                    }
                }
            }
        }
        kept
    }

    /// Take the self-test's runs that came: `Some(true)` if one did,
    /// `None` if its subscription can no longer be read (it is said, and
    /// the caller makes it again).
    fn hear_selftest(&mut self, tests: &mut Subscriber) -> Option<bool> {
        let mut heard = false;
        loop {
            match tests.recv(Duration::ZERO) {
                Ok(Some(message)) => match message.parse::<SelfTest>() {
                    Ok(test) if test.check().is_ok() => {
                        self.selftest.take(&test);
                        heard = true;
                    }
                    _ => (self.say)(Note::SelfTestUnreadable),
                },
                Ok(None) => return Some(heard),
                Err(error) => {
                    (self.say)(Note::NoSelfTest { kind: error.kind() });
                    return None;
                }
            }
        }
    }

    /// Work the lamp's health out, say so if its level is another, and
    /// publish it if it is news or its time has come.
    fn work_out(&mut self, now: Instant) {
        let options = self.options;
        let off: Vec<(String, String)> = options
            .needs
            .iter()
            .filter(|(_, path)| !path.exists())
            .map(|(name, path)| (name.clone(), path.display().to_string()))
            .collect();
        let expected: Vec<String> = options
            .expected
            .iter()
            .filter(|name| !off.iter().any(|(off, _)| off == *name))
            .cloned()
            .collect();
        let mut health = parts::health(self.parts.reports(), now, &expected, options.stale_after);
        parts::mark_off(&mut health, &off);
        if let Some(item) = self.selftest.item(unix_now()) {
            parts::mark_selftest(&mut health, item);
        }
        if self.level != Some(health.level) {
            (self.say)(Note::Health {
                from: self.level,
                to: health.level,
                message: health.message.clone(),
            });
            self.level = Some(health.level);
        }
        let news = self
            .published
            .as_ref()
            .is_none_or(|before| !parts::same_but_for_ages(before, &health, options.stale_after));
        if !news && now < self.health_due {
            return;
        }
        let at_us = self.at_us();
        if self.health_out.publish(at_us, &health, self.say) {
            self.totals.healths += 1;
            self.published = Some(health);
            self.health_due = now + options.times.health_every;
        } else {
            // It stays news, and is tried again with the next working out.
            self.totals.unpublished += 1;
            self.health_due = now + options.times.report_every;
        }
    }
}

/// The wall clock's seconds since the Unix epoch, as a run's `started` is.
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// Whether something done every `every` is due at `now`.
fn due(last: Option<Instant>, every: Duration, now: Instant) -> bool {
    last.is_none_or(|at| now.saturating_duration_since(at) >= every)
}

/// How long until something done every `every` is due.
fn until(last: Option<Instant>, every: Duration, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |at| {
        every.saturating_sub(now.saturating_duration_since(at))
    })
}

/// Serve the lamp's health on `bus` until `stop` is set: listen to every
/// part's report, read the board with `read`, and publish `lamp/health`,
/// `board/state` and the board's own report.
///
/// It fails only at the start, if it cannot subscribe. After that nothing
/// ends it but `stop`. `say` is given each [`Note`] as it happens.
pub fn run(
    bus: &Bus,
    options: &Options,
    read: &mut dyn FnMut() -> Reading,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
) -> io::Result<Totals> {
    let mut ear = bus.subscribe_typed(Diagnostic::TOPIC, NAME, Diagnostic::TYPE)?;
    // The self-test's runs, the newest first as the topic keeps it. Not
    // being able to hear them costs only their item, never the health: it
    // is said once and tried again each report's time.
    let mut tests = bus
        .subscribe_typed(SelfTest::TOPIC, NAME, SelfTest::TYPE)
        .map_err(|error| say(Note::NoSelfTest { kind: error.kind() }))
        .ok();
    let times = options.times;
    let mut serve = Serve {
        options,
        say,
        started: Instant::now(),
        parts: Parts::new(),
        board_report: None,
        unread: String::new(),
        level: None,
        published: None,
        health_due: Instant::now(),
        health_out: Out::new(Health::TOPIC, Health::declaration()),
        board_out: Out::new(BoardState::TOPIC, BoardState::declaration()),
        reports_out: Out::new(Diagnostic::TOPIC, Diagnostic::declaration()),
        online_out: Out::new(Online::TOPIC, Online::declaration()),
        reach: options
            .reach
            .clone()
            .filter(|_| options.online)
            .map(crate::reach::Reach::new)
            .map(|reach| {
                if options.reach_idle {
                    reach.idle()
                } else {
                    reach
                }
            }),
        unreadable: Trouble::default(),
        unreadable_at: None,
        receiving: Trouble::default(),
        passed_over: 0,
        full: false,
        totals: Totals::default(),
        selftest: parts::SelfTests::default(),
    };
    let (mut board_read, mut reported): (Option<Instant>, Option<Instant>) = (None, None);
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let mut work_out = false;
        if due(reported, times.report_every, now) {
            // Before anything is published this turn: a topic that could
            // not be published is tried again here, once in this time.
            for out in [
                &mut serve.health_out,
                &mut serve.board_out,
                &mut serve.reports_out,
            ] {
                out.make(bus, serve.say);
            }
            if options.online {
                serve.online_out.make(bus, serve.say);
            }
        }
        if due(board_read, times.board_every, now) {
            board_read = Some(now);
            serve.read_board(read);
        }
        if due(reported, times.report_every, now) {
            if tests.is_none() {
                tests = bus
                    .subscribe_typed(SelfTest::TOPIC, NAME, SelfTest::TYPE)
                    .ok();
            }
            reported = Some(now);
            serve.report_board(now);
            serve.report_online(now);
            work_out = true;
        }
        // The wait of the loop is here: for a report, and no longer than
        // until the next thing is due.
        let now = Instant::now();
        let wait = times
            .tick
            .min(until(board_read, times.board_every, now))
            .min(until(reported, times.report_every, now))
            .min(serve.health_due.saturating_duration_since(now));
        work_out |= serve.listen(&mut ear, wait);
        if let Some(subscribed) = tests.as_mut() {
            match serve.hear_selftest(subscribed) {
                Some(heard) => work_out |= heard,
                // Made again at the next report's time.
                None => tests = None,
            }
        }
        let now = Instant::now();
        serve.only_reports(now);
        if work_out || now >= serve.health_due {
            serve.work_out(now);
        }
        for out in [
            &mut serve.health_out,
            &mut serve.board_out,
            &mut serve.reports_out,
        ] {
            out.alive(serve.say);
        }
        if options.online {
            serve.online_out.alive(serve.say);
        }
    }
    Ok(serve.totals)
}

/// What [`run`] is given to read the board with on a lamp.
/// It keeps a [`board::Watch`]: how long the processors have been held
/// back, and which processes used them since the last reading.
pub fn board_reader(places: board::Places) -> impl FnMut() -> Reading {
    let mut watch = board::Watch::default();
    move || {
        let mut reading = board::read(&places);
        watch.observe(&mut reading, Instant::now());
        reading
    }
}
