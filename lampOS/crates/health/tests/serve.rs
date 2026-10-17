//! The loop on a real bus, in a directory of the test's own: the test
//! plays the lamp's parts, hands the loop the board it is to read, and
//! reads what the loop publishes and says about itself.
mod tree;

use lamp_bus::{Bus, Kind, Message, Publisher, Subscriber};
use lamp_health::{
    board::{Reading, Unread, Zone},
    parts::same_but_for_ages,
    serve::{self, Note, Options, Times, Totals},
};
use lamp_msgs::{
    BoardState, Diagnostic, Health, HealthItem, Level, Occasion, PartResult, SelfTest, Verdict,
};
use serde_json::json;
use std::{
    fs, io,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};
use tree::Tree;

/// How long a test waits for something that is to happen. The machine
/// these run on is often busy: what is waited for comes in milliseconds,
/// and the limit is for when it does not come at all.
const WAIT: Duration = Duration::from_secs(20);
/// A stale time nothing in a test reaches unless the test is about it.
const NEVER: Duration = Duration::from_secs(600);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A board with nothing the matter: the real one's numbers, idle.
fn well() -> Reading {
    Reading {
        zones: vec![
            Zone {
                name: "cpul_thermal_zone".into(),
                milli_c: 63_500,
                passive_milli_c: vec![70_000, 90_000],
                critical_milli_c: Some(110_000),
            },
            Zone {
                name: "skin_zone".into(),
                milli_c: 37_000,
                passive_milli_c: Vec::new(),
                critical_milli_c: None,
            },
        ],
        cpu_load: Some(0.31),
        memory_used: Some(0.44),
        disk_free: Some(0.5),
        uptime_s: Some(208_800),
        wifi_dbm: None,
        clock_synced: None,
        unread: Vec::new(),
        judged: Vec::new(),
        cpu_caps: Vec::new(),
        throttled_for: None,
        busiest: Vec::new(),
        processes: Vec::new(),
        cpu_ticks: None,
        cpufreq_cooling: false,
        hot_held: false,
    }
}

fn item<'a>(health: &'a Health, name: &str) -> Option<&'a HealthItem> {
    health.items.iter().find(|item| item.name == name)
}

fn level_of(health: &Health, name: &str) -> Option<Level> {
    item(health, name).map(|item| item.level)
}

fn shape(health: &Health) -> Vec<(&str, Level, &str)> {
    health
        .items
        .iter()
        .map(|item| (item.name.as_str(), item.level, item.message.as_str()))
        .collect()
}

/// The notes about the lamp's level, as `(from, to, message)`.
fn levels(notes: &[Note]) -> Vec<(Option<Level>, Level, &str)> {
    notes
        .iter()
        .filter_map(|note| match note {
            Note::Health { from, to, message } => Some((*from, *to, message.as_str())),
            _ => None,
        })
        .collect()
}

type Reader = Box<dyn FnMut() -> Reading + Send>;

/// The loop running on a bus of the test's own, and the test as the
/// lamp's parts on the same bus.
struct Rig {
    root: PathBuf,
    bus: Bus,
    stop: Arc<AtomicBool>,
    running: Option<thread::JoinHandle<io::Result<Totals>>>,
    /// What the loop says about itself, as it says it, and what of that
    /// the test has taken in so far.
    saying: mpsc::Receiver<Note>,
    notes: Vec<Note>,
    /// The board the loop reads, unless the test gave it a reader.
    reading: Arc<Mutex<Reading>>,
    health: Subscriber,
    board: Subscriber,
}

impl Rig {
    fn new(name: &str) -> Self {
        Self::with(name, |_| {}, None, |_| {})
    }

    /// `change` is given the options before the loop starts: by default
    /// nothing goes stale, the board reports twenty times a second and is
    /// read ten times, and the health is published at least every 300 ms.
    /// `before` is given the rig before the loop starts.
    fn with(
        name: &str,
        change: impl FnOnce(&mut Options),
        reader: Option<Reader>,
        before: impl FnOnce(&Self),
    ) -> Self {
        // A root short enough for a socket's path, which the system's own
        // temporary directory on a Mac is not.
        let root = PathBuf::from(format!("/tmp/lh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // Publishers look for new subscribers every 20 ms, not 200.
        let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
        let mut options = Options {
            expected: Vec::new(),
            stale_after: NEVER,
            times: Times {
                tick: ms(10),
                report_every: ms(50),
                health_every: ms(300),
                board_every: ms(100),
            },
            online: false,
            reach: None,
            needs: Vec::new(),
        };
        change(&mut options);
        let (said_so, saying) = mpsc::channel();
        let mut rig = Self {
            // The test listens before the loop has anything to say.
            health: bus
                .subscribe_typed(Health::TOPIC, "test", Health::TYPE)
                .unwrap(),
            board: bus
                .subscribe_typed(BoardState::TOPIC, "test", BoardState::TYPE)
                .unwrap(),
            stop: Arc::new(AtomicBool::new(false)),
            running: None,
            saying,
            notes: Vec::new(),
            reading: Arc::new(Mutex::new(well())),
            root,
            bus,
        };
        before(&rig);
        let (bus, stop) = (rig.bus.clone(), Arc::clone(&rig.stop));
        let reading = Arc::clone(&rig.reading);
        let mut reader =
            reader.unwrap_or_else(|| Box::new(move || reading.lock().unwrap().clone()));
        rig.running = Some(thread::spawn(move || {
            serve::run(&bus, &options, &mut reader, &stop, &mut |note| {
                let _ = said_so.send(note);
            })
        }));
        // The loop has subscribed once its socket is there.
        let started = Instant::now();
        while !rig
            .bus
            .subscribers(Diagnostic::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.socket.starts_with("health.") && listener.listening)
        {
            assert!(started.elapsed() < WAIT, "the loop did not subscribe");
            thread::sleep(ms(5));
        }
        rig
    }

    /// A part of the lamp, as the parts publish: the topic declared.
    fn part(&self, name: &str) -> Publisher {
        self.bus
            .publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), name)
            .unwrap()
    }

    /// The board the loop reads from now on.
    fn board_is(&self, change: impl FnOnce(&mut Reading)) {
        change(&mut self.reading.lock().unwrap());
    }

    /// The next health the loop publishes, and the message it came in.
    fn next(&mut self) -> (Health, Message) {
        let message = self
            .health
            .recv(WAIT)
            .unwrap()
            .expect("a health on the bus");
        assert_eq!(message.header.source, "health");
        assert_eq!(message.header.type_name.as_deref(), Some(Health::TYPE));
        // It means nothing fifteen seconds after it was written.
        assert_eq!(message.header.expires_ms, Some(15_000));
        let health: Health = message.parse().unwrap();
        assert_eq!(health.check(), Ok(()), "{health:?}");
        (health, message)
    }

    /// The first health for which `wanted` holds. Every health on the way
    /// to it is checked.
    fn health_where(&mut self, what: &str, wanted: impl Fn(&Health) -> bool) -> Health {
        let started = Instant::now();
        loop {
            let (health, _) = self.next();
            if wanted(&health) {
                return health;
            }
            assert!(
                started.elapsed() < WAIT,
                "no health where {what}; the last was {health:?}"
            );
        }
    }

    /// The next of the board's numbers the loop publishes.
    fn numbers(&mut self) -> BoardState {
        let message = self
            .board
            .recv(WAIT)
            .unwrap()
            .expect("the board's numbers on the bus");
        assert_eq!(message.header.source, "health");
        assert_eq!(message.header.type_name.as_deref(), Some(BoardState::TYPE));
        assert_eq!(message.header.expires_ms, Some(15_000));
        let state: BoardState = message.parse().unwrap();
        assert_eq!(state.check(), Ok(()), "{state:?}");
        state
    }

    /// Wait until the loop says something for which `wanted` holds.
    fn said(&mut self, what: &str, wanted: impl Fn(&Note) -> bool) {
        let started = Instant::now();
        loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let Ok(note) = self.saying.recv_timeout(left) else {
                panic!("the loop did not say {what}; it said {:?}", self.notes);
            };
            let found = wanted(&note);
            self.notes.push(note);
            if found {
                return;
            }
        }
    }

    /// Stop the loop: what it counted, and everything it said. The bus
    /// stays as the loop left it until the test ends.
    fn stop(&mut self) -> (Totals, Vec<Note>) {
        self.stop.store(true, Ordering::Relaxed);
        let totals = self.running.take().unwrap().join().unwrap().unwrap();
        let mut notes = std::mem::take(&mut self.notes);
        notes.extend(self.saying.try_iter());
        (totals, notes)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(running) = self.running.take() {
            let _ = running.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// One report from a part, taken by the loop's socket.
fn report(part: &mut Publisher, level: Level, name: &str, message: &str) {
    let sent = part
        .publish_json(
            0,
            &Diagnostic {
                level,
                name: name.into(),
                message: message.into(),
                values: Vec::new(),
            },
        )
        .unwrap();
    assert!(sent.delivered >= 1 && sent.dropped == 0, "{name}: {sent:?}");
}

#[test]
fn reports_arrive_and_the_lamps_health_says_so() {
    let mut rig = Rig::new("arrive");
    // Before any part has reported: the board alone, and it is well.
    let (alone, _) = rig.next();
    assert_eq!(shape(&alone), [("board", Level::Ok, "")]);
    assert_eq!((alone.level, alone.message.as_str()), (Level::Ok, ""));
    assert!(alone.missing.is_empty());
    assert_eq!(
        rig.bus.declaration(Health::TOPIC).unwrap(),
        Some(Health::declaration())
    );
    assert_eq!(rig.bus.publishers(Health::TOPIC).unwrap()[0].name, "health");

    let (mut voice, mut body) = (rig.part("voice"), rig.part("body"));
    report(&mut voice, Level::Ok, "voice", "");
    report(&mut body, Level::Warn, "body", "yaw is hot");
    let health = rig.health_where("both parts are in it", |health| health.items.len() == 3);
    // Worst first, then by name.
    assert_eq!(
        shape(&health),
        [
            ("body", Level::Warn, "yaw is hot"),
            ("board", Level::Ok, ""),
            ("voice", Level::Ok, "")
        ]
    );
    assert_eq!(
        (health.level, health.message.as_str()),
        (Level::Warn, "body: yaw is hot")
    );
    assert!(
        health.items.iter().all(|item| item.age_ms < 20_000),
        "{health:?}"
    );

    report(
        &mut voice,
        Level::Error,
        "voice",
        "the link to the service is down",
    );
    let health = rig.health_where("the voice's error is in it", |health| {
        health.level == Level::Error
    });
    assert_eq!(
        shape(&health),
        [
            ("voice", Level::Error, "the link to the service is down"),
            ("body", Level::Warn, "yaw is hot"),
            ("board", Level::Ok, "")
        ]
    );
    assert_eq!(health.message, "voice: the link to the service is down");

    report(&mut voice, Level::Ok, "voice", "");
    report(&mut body, Level::Ok, "body", "");
    let health = rig.health_where("all is well again", |health| health.level == Level::Ok);
    assert_eq!(health.message, "");
    assert_eq!(health.items.len(), 3);

    let (totals, notes) = rig.stop();
    // The parts' five, and the board's own, which come all the time.
    assert!(totals.reports >= 6, "{totals:?}");
    assert_eq!(
        (totals.unreadable, totals.turned_away, totals.unpublished),
        (0, 0, 0)
    );
    assert!(totals.healths >= 4 && totals.boards >= 1, "{totals:?}");
    // What it said: the lamp's level each time it became another, and
    // nothing else. Not a line for any message.
    let levels = levels(&notes);
    assert_eq!(levels.len(), notes.len(), "{notes:?}");
    assert_eq!(
        levels[..3],
        [
            (None, Level::Ok, ""),
            (Some(Level::Ok), Level::Warn, "body: yaw is hot"),
            (
                Some(Level::Warn),
                Level::Error,
                "voice: the link to the service is down"
            ),
        ]
    );
    // From the error to well, by way of the warning if the two last
    // reports were taken a turn apart.
    assert!(levels.len() == 4 || levels.len() == 5, "{levels:?}");
    assert_eq!(levels.last().map(|level| level.1), Some(Level::Ok));
}

#[test]
fn a_part_that_goes_quiet_is_stale_and_is_itself_again_when_it_comes_back() {
    let mut rig = Rig::with(
        "quiet",
        |options| options.stale_after = ms(600),
        None,
        |_| {},
    );
    rig.next();
    let mut voice = rig.part("voice");
    report(&mut voice, Level::Ok, "voice", "");
    let fresh = rig.health_where("the voice is in it", |health| {
        level_of(health, "voice") == Some(Level::Ok)
    });
    assert!(item(&fresh, "voice").unwrap().age_ms <= 600);

    // It says nothing more. The board goes on reporting.
    let quiet = rig.health_where("the voice is stale", |health| {
        level_of(health, "voice") == Some(Level::Stale)
    });
    let silent = item(&quiet, "voice").unwrap();
    assert!(silent.age_ms > 600, "{quiet:?}");
    assert_eq!(
        silent.message,
        format!("not heard from for {} s", silent.age_ms / 1_000)
    );
    // The board is well unless the machine held the loop up for longer
    // than the stale time, which a busy one may.
    if level_of(&quiet, "board") == Some(Level::Ok) {
        assert_eq!(quiet.level, Level::Stale);
        assert_eq!(quiet.message, format!("voice: {}", silent.message));
        assert_eq!(quiet.items[0].name, "voice");
    }

    // It comes back, with something to say.
    report(&mut voice, Level::Warn, "voice", "slow to answer");
    let back = rig.health_where("the voice is back", |health| {
        level_of(health, "voice") == Some(Level::Warn)
    });
    let returned = item(&back, "voice").unwrap();
    assert_eq!(returned.message, "slow to answer");
    assert!(returned.age_ms <= 600, "{back:?}");
    assert_eq!(back.level, Level::Warn);

    let (_, notes) = rig.stop();
    let levels = levels(&notes);
    assert_eq!(levels[0], (None, Level::Ok, ""));
    let went = levels
        .iter()
        .position(|(_, to, message)| {
            *to == Level::Stale && message.starts_with("voice: not heard from for ")
        })
        .expect("the lamp was said to be stale");
    assert!(
        levels[went..].contains(&(Some(Level::Stale), Level::Warn, "voice: slow to answer")),
        "{levels:?}"
    );
}

#[test]
fn an_expected_part_that_never_reports_is_missing_until_it_does() {
    let mut rig = Rig::with(
        "missing",
        |options| options.expected = vec!["voice".into(), "eyes".into()],
        None,
        |_| {},
    );
    let (none, _) = rig.next();
    assert_eq!(none.missing, ["voice", "eyes"]);
    assert_eq!(shape(&none), [("board", Level::Ok, "")]);
    assert_eq!(
        (none.level, none.message.as_str()),
        (Level::Stale, "voice: has never reported")
    );

    let mut voice = rig.part("voice");
    report(&mut voice, Level::Ok, "voice", "");
    let one = rig.health_where("the voice has reported", |health| health.missing.len() == 1);
    assert_eq!(one.missing, ["eyes"]);
    assert_eq!(
        (one.level, one.message.as_str()),
        (Level::Stale, "eyes: has never reported")
    );
    assert_eq!(level_of(&one, "voice"), Some(Level::Ok));

    // A part nobody expected is an item like the others, and changes
    // nothing about what is missing.
    let mut ring = rig.part("ring");
    report(&mut ring, Level::Ok, "ring", "");
    let more = rig.health_where("the ring is in it", |health| item(health, "ring").is_some());
    assert_eq!(more.missing, ["eyes"]);

    let mut eyes = rig.part("eyes");
    report(&mut eyes, Level::Ok, "eyes", "");
    let all = rig.health_where("nothing is missing", |health| health.missing.is_empty());
    assert_eq!((all.level, all.message.as_str()), (Level::Ok, ""));
    assert_eq!(all.items.len(), 4);

    let (_, notes) = rig.stop();
    // The level was stale throughout the missing, whichever part it was.
    assert_eq!(
        levels(&notes),
        [
            (None, Level::Stale, "voice: has never reported"),
            (Some(Level::Stale), Level::Ok, "")
        ]
    );
}

#[test]
fn the_boards_numbers_are_read_from_its_files_and_published() {
    let tree = Tree::real_board("served");
    let reader: Reader = Box::new(serve::board_reader(tree.places()));
    let mut rig = Rig::with("tree", |_| {}, Some(reader), |_| {});
    let state = rig.numbers();
    assert_eq!(state.temperature_c.len(), 8);
    assert_eq!(state.temperature_c["cpul_thermal_zone"], 63.5);
    assert_eq!(state.temperature_c["skin_zone"], 37.0);
    assert!((state.cpu_load - 0.31).abs() < 1e-6);
    assert!((state.memory_used - 0.44).abs() < 1e-6);
    assert_eq!(state.uptime_s, 208_800);
    assert_eq!((state.wifi_dbm, state.clock_synced), (None, None));
    // The disk is the machine's own.
    assert!((state.disk_free - tree::disk_free(&tree.path("disk"))).abs() < 0.01);
    assert_eq!(
        rig.bus.declaration(BoardState::TOPIC).unwrap(),
        Some(BoardState::declaration())
    );
    // The level the machine's disk allows the board: ok unless it is
    // nearly full.
    let disk_is_fine = state.disk_free >= lamp_health::board::DISK_FREE_WARN;

    // The board's own report, as any subscriber of `diagnostics` has it.
    let mut reports = rig
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let message = reports.recv(WAIT).unwrap().expect("the board's report");
    assert_eq!(message.header.source, "health");
    assert_eq!(message.header.type_name.as_deref(), Some(Diagnostic::TYPE));
    let own: Diagnostic = message.parse().unwrap();
    assert_eq!(own.name, "board");
    let value = |key: &str| {
        own.values
            .iter()
            .find(|value| value.key == key)
            .map(|value| value.value.as_str())
    };
    assert_eq!(value("cpu_load"), Some("0.31"));
    assert_eq!(value("uptime_s"), Some("208800"));
    assert_eq!(value("hottest_zone"), Some("ddr_thermal_zone"));
    assert_eq!(value("hottest_c"), Some("64.5"));
    if disk_is_fine {
        assert_eq!((own.level, own.message.as_str()), (Level::Ok, ""));
    }
    // And in the lamp's health.
    let health = rig.health_where("the board is in it", |health| {
        item(health, "board").is_some()
    });
    if disk_is_fine {
        assert_eq!(shape(&health), [("board", Level::Ok, "")]);
    }

    // A processor zone passes the point where the board slows itself
    // down: the next reading says so everywhere.
    tree.heat(0, 72_000);
    let warm = "cpul_thermal_zone is at 72 °C: the board slows itself down from 70";
    let health = rig.health_where("the board is warm", |health| {
        item(health, "board").is_some_and(|board| board.level != Level::Ok && board.message == warm)
    });
    assert_eq!(
        (health.level, health.message),
        (Level::Warn, format!("board: {warm}"))
    );
    let started = Instant::now();
    while rig.numbers().temperature_c["cpul_thermal_zone"] != 72.0 {
        assert!(started.elapsed() < WAIT, "the numbers stayed as they were");
    }
    let started = Instant::now();
    loop {
        let own: Diagnostic = reports.recv(WAIT).unwrap().unwrap().parse().unwrap();
        if own.message == warm {
            assert_eq!(own.level, Level::Warn);
            break;
        }
        assert!(started.elapsed() < WAIT, "the report stayed as it was");
    }
    let (totals, _) = rig.stop();
    assert_eq!(totals.unpublished, 0);
}

#[test]
fn a_health_that_has_not_changed_is_published_again_within_its_period() {
    let mut rig = Rig::with(
        "again",
        |options| options.times.health_every = ms(400),
        None,
        |_| {},
    );
    let started = Instant::now();
    let (first, one) = rig.next();
    let (second, two) = rig.next();
    let (third, three) = rig.next();
    // Three messages, one after another, and not one read three times.
    assert_eq!(
        (one.header.seq, two.header.seq, three.header.seq),
        (1, 2, 3)
    );
    assert_eq!((one.gap, two.gap, three.gap), (0, 0, 0));
    // The same but for the board's age.
    for health in [&first, &second, &third] {
        assert_eq!(shape(health), [("board", Level::Ok, "")]);
        assert!(same_but_for_ages(&first, health, NEVER));
    }
    // Each about a period after the one before, by the publisher's own
    // clock, and not with every turn of the loop, which is forty times as
    // often. The line is at half the period, to leave a busy machine room.
    let apart = |a: &Message, b: &Message| b.header.at_us.saturating_sub(a.header.at_us);
    for gap in [apart(&one, &two), apart(&two, &three)] {
        assert!(gap >= 200_000, "{gap} us");
    }
    assert!(started.elapsed() < WAIT);
    let (totals, notes) = rig.stop();
    assert!(totals.healths >= 3);
    // Said once, however often it was published.
    assert_eq!(levels(&notes), [(None, Level::Ok, "")]);
}

#[test]
fn reports_that_say_nothing_new_publish_nothing_and_a_change_is_published_at_once() {
    // The health's own period is far off: only news is published.
    let mut rig = Rig::with(
        "news",
        |options| options.times.health_every = NEVER,
        None,
        |_| {},
    );
    let (_, first) = rig.next();
    assert_eq!(first.header.seq, 1);
    let mut voice = rig.part("voice");
    report(&mut voice, Level::Ok, "voice", "");
    let (with_voice, second) = rig.next();
    assert_eq!(second.header.seq, 2);
    assert_eq!(
        shape(&with_voice),
        [("board", Level::Ok, ""), ("voice", Level::Ok, "")]
    );
    // The same report ten times more, while the board reports as well.
    for _ in 0..10 {
        report(&mut voice, Level::Ok, "voice", "");
        thread::sleep(ms(20));
    }
    assert!(rig.health.recv(ms(400)).unwrap().is_none());
    // A change is published without waiting for anything: the next
    // message on the topic is the one that has it.
    report(&mut voice, Level::Warn, "voice", "slow to answer");
    let (changed, third) = rig.next();
    assert_eq!(third.header.seq, 3);
    assert_eq!(
        (changed.level, changed.message.as_str()),
        (Level::Warn, "voice: slow to answer")
    );
    // So is a change of the message alone.
    report(&mut voice, Level::Warn, "voice", "very slow to answer");
    let (reworded, fourth) = rig.next();
    assert_eq!(fourth.header.seq, 4);
    assert_eq!(reworded.message, "voice: very slow to answer");
    let (totals, _) = rig.stop();
    assert_eq!(totals.healths, 4);
    assert!(totals.reports >= 13, "{totals:?}");
}

/// When the entry of the loop's publisher of `topic` was last given a new
/// time: how a publisher shows the bus that it is alive.
fn shown(rig: &Rig, topic: &str) -> SystemTime {
    let pubs = rig.root.join(topic).join("pubs");
    let entry = fs::read_dir(&pubs)
        .unwrap()
        .map(|entry| entry.unwrap())
        .find(|entry| entry.file_name().to_str().unwrap().starts_with("health."))
        .unwrap_or_else(|| panic!("no entry for health in {}", pubs.display()));
    entry.metadata().unwrap().modified().unwrap()
}

#[test]
fn between_two_messages_the_publishers_show_that_they_are_alive() {
    // Nothing is published after the first of each: the next is ten
    // minutes off. The bus has the lamp's own lease of three seconds, with
    // which a publisher shows itself once a second.
    let far = |options: &mut Options| {
        options.times.health_every = NEVER;
        options.times.board_every = NEVER;
    };
    let mut rig = Rig::with("alive", far, None, |_| {});
    rig.next();
    rig.numbers();
    let topics = [Health::TOPIC, BoardState::TOPIC];
    let mut last = topics.map(|topic| shown(&rig, topic));
    let mut times = [0, 0];
    let started = Instant::now();
    while times.iter().any(|times| *times < 2) {
        assert!(
            started.elapsed() < WAIT,
            "shown alive {times:?} times in {WAIT:?}"
        );
        thread::sleep(ms(50));
        for (at, topic) in topics.iter().enumerate() {
            let now = shown(&rig, topic);
            if now > last[at] {
                times[at] += 1;
                last[at] = now;
            }
        }
    }
    // And no message came meanwhile: it was `alive()`, not a publish.
    assert!(rig.health.recv(ms(50)).unwrap().is_none());
    assert!(rig.board.recv(ms(50)).unwrap().is_none());
    // It is listed as the publisher of all three of its topics.
    for topic in [Health::TOPIC, BoardState::TOPIC, Diagnostic::TOPIC] {
        let listed = rig.bus.publishers(topic).unwrap();
        assert!(
            listed.iter().any(|source| source.name == "health"),
            "{topic}: {listed:?}"
        );
    }
    let (totals, _) = rig.stop();
    assert_eq!((totals.healths, totals.boards), (1, 1));
    // Stopped, it is listed no longer, without a lease having to run out.
    for topic in [Health::TOPIC, BoardState::TOPIC, Diagnostic::TOPIC] {
        assert!(rig.bus.publishers(topic).unwrap().is_empty(), "{topic}");
    }
    // What it kept stays, for as long as it means anything.
    assert!(rig.bus.latest(Health::TOPIC).unwrap().is_some());
}

#[test]
fn what_is_not_a_report_is_counted_passed_over_and_said_once() {
    // A stale time of two seconds: so long after the last of them, the
    // loop says that such messages have stopped. Long enough that a busy
    // machine does not make two troubles of one by holding the test up
    // between two of its messages.
    let mut rig = Rig::with(
        "unreadable",
        |options| options.stale_after = ms(2_000),
        None,
        |_| {},
    );
    rig.next();
    // A publisher that declared nothing can send anything.
    let mut odd = rig
        .bus
        .publisher(Diagnostic::TOPIC, Kind::Event, "odd")
        .unwrap();
    let mut voice = rig.part("voice");
    let wrong = [
        json!({"level": "purple", "name": "voice", "message": "", "values": []}),
        json!({"name": "voice"}),
        json!("the voice is fine"),
        // A report in every way but its name, which no part can have.
        json!({"level": "error", "name": "vo\nice", "message": "x", "values": []}),
        json!({"level": "error", "name": "", "message": "x", "values": []}),
    ];
    for wrong in &wrong {
        assert!(odd.publish_json(0, wrong).unwrap().delivered >= 1);
        // Among them a part that reports well, as the board does all the
        // while: neither ends the trouble.
        report(&mut voice, Level::Ok, "voice", "");
    }
    // And something that is no message of the bus at all, straight into
    // the loop's socket.
    let socket = fs::read_dir(rig.root.join("diagnostics/subs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            let name = path.file_name().unwrap().to_str().unwrap();
            name.starts_with("health.") && name.ends_with(".sock")
        })
        .expect("the loop's socket");
    UnixDatagram::unbound()
        .unwrap()
        .send_to(b"not a message", &socket)
        .unwrap();
    // None of it is in the lamp's health, and the report that came with
    // it is.
    report(&mut voice, Level::Ok, "voice", "");
    let health = rig.health_where("the voice is in it", |health| {
        level_of(health, "voice") == Some(Level::Ok)
    });
    assert_eq!(health.items.len(), 2, "{health:?}");
    assert!(item(&health, "board").is_some());
    // Once they have stopped for the stale time, that is said, with how
    // many there were.
    rig.said("that they have stopped", |note| {
        matches!(note, Note::Readable { .. })
    });
    let kept: Health = rig
        .bus
        .latest(Health::TOPIC)
        .unwrap()
        .expect("a kept health")
        .parse()
        .unwrap();
    let mut names: Vec<&str> = kept.items.iter().map(|item| item.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["board", "voice"]);
    let (totals, notes) = rig.stop();
    assert_eq!(totals.unreadable, 6);
    let about: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::Unreadable | Note::Readable { .. }))
        .collect();
    assert_eq!(
        about,
        [&Note::Unreadable, &Note::Readable { passed_over: 6 }]
    );
}

#[test]
fn a_board_that_cannot_be_read_is_said_once_and_its_numbers_are_not_published() {
    let mut rig = Rig::new("unread");
    rig.numbers();
    rig.board_is(|board| {
        board.cpu_load = None;
        board.unread = vec![Unread {
            what: "cpu_load",
            why: "/proc/loadavg: gone".into(),
        }];
    });
    // The board's report says which number, and the lamp has a warning.
    let health = rig.health_where("the board says what it could not read", |health| {
        level_of(health, "board") == Some(Level::Warn)
    });
    assert_eq!(
        (health.level, health.message.as_str()),
        (Level::Warn, "board: could not be read: cpu_load")
    );
    // What was published before the number went is taken off the socket;
    // after that, over four readings, nothing comes.
    while rig.board.recv(ms(50)).unwrap().is_some() {}
    assert!(rig.board.recv(ms(400)).unwrap().is_none());

    // A second number goes: said again, as what is not read now.
    rig.board_is(|board| {
        board.uptime_s = None;
        board.unread.push(Unread {
            what: "uptime_s",
            why: "/proc/uptime: gone".into(),
        });
    });
    rig.health_where("the board says both", |health| {
        health.message == "board: could not be read: cpu_load, uptime_s"
    });

    // Both are back: the numbers are published again and the lamp is well.
    rig.board_is(|board| *board = well());
    assert_eq!(rig.numbers().uptime_s, 208_800);
    rig.health_where("the lamp is well", |health| health.level == Level::Ok);
    let (totals, notes) = rig.stop();
    assert_eq!(totals.unpublished, 0);
    let about: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::BoardUnread { .. } | Note::BoardRead))
        .collect();
    assert_eq!(
        about,
        [
            &Note::BoardUnread {
                what: "cpu_load (/proc/loadavg: gone)".into()
            },
            &Note::BoardUnread {
                what: "cpu_load (/proc/loadavg: gone); uptime_s (/proc/uptime: gone)".into()
            },
            &Note::BoardRead,
        ]
    );
    assert_eq!(
        levels(&notes),
        [
            (None, Level::Ok, ""),
            (
                Some(Level::Ok),
                Level::Warn,
                "board: could not be read: cpu_load"
            ),
            (Some(Level::Warn), Level::Ok, ""),
        ]
    );
}

#[test]
fn a_topic_that_cannot_be_published_is_said_once_and_the_rest_goes_on() {
    let mut rig = Rig::with(
        "blocked",
        |_| {},
        None,
        |rig| {
            // A file where the directory of `lamp/health` would be.
            fs::remove_dir_all(rig.root.join("lamp")).unwrap();
            fs::write(rig.root.join("lamp"), b"in the way").unwrap();
        },
    );
    // The board's numbers and its report are published all the same.
    assert_eq!(rig.numbers().uptime_s, 208_800);
    let mut reports = rig
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let own: Diagnostic = reports.recv(WAIT).unwrap().unwrap().parse().unwrap();
    assert_eq!(own.name, "board");
    // The way is cleared. Within a turn or two the health is published,
    // and the test, whose own socket went with the file, listens again.
    fs::remove_file(rig.root.join("lamp")).unwrap();
    rig.health = rig
        .bus
        .subscribe_typed(Health::TOPIC, "test", Health::TYPE)
        .unwrap();
    let (health, _) = rig.next();
    assert_eq!(shape(&health), [("board", Level::Ok, "")]);
    let (totals, notes) = rig.stop();
    let about: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::NotPublishing { .. } | Note::Publishing { .. }))
        .collect();
    assert_eq!(about.len(), 2, "{notes:?}");
    assert_eq!(
        about[0],
        &Note::NotPublishing {
            topic: "lamp/health",
            kind: io::ErrorKind::NotADirectory
        }
    );
    let Note::Publishing { topic, unpublished } = about[1] else {
        panic!("{about:?}");
    };
    assert_eq!(*topic, "lamp/health");
    // Every health that went nowhere, and nothing of the other topics.
    assert!(*unpublished >= 1);
    assert_eq!(totals.unpublished, *unpublished);
    assert!(totals.healths >= 1 && totals.boards >= 1);
    // The lamp's level was said though it could not be published. The
    // self-test's runs, whose directory was in the way too, were said
    // once as not heard, and the health went on without them.
    assert_eq!(levels(&notes), [(None, Level::Ok, "")]);
    assert_eq!(
        notes[0],
        Note::NoSelfTest {
            kind: io::ErrorKind::NotADirectory
        },
        "{notes:?}"
    );
    assert!(matches!(notes[1], Note::NotPublishing { .. }), "{notes:?}");
}

#[test]
fn every_note_is_one_line_about_the_program_or_the_lamps_level() {
    let lines: Vec<String> = [
        Note::Health {
            from: None,
            to: Level::Ok,
            message: String::new(),
        },
        Note::Health {
            from: Some(Level::Ok),
            to: Level::Warn,
            message: "board: cpul_thermal_zone is at 66 °C: the board slows itself down from 70"
                .into(),
        },
        Note::Health {
            from: Some(Level::Warn),
            to: Level::Ok,
            message: String::new(),
        },
        Note::Health {
            from: None,
            to: Level::Stale,
            message: "voice: has never reported".into(),
        },
        Note::NotPublishing {
            topic: "lamp/health",
            kind: io::ErrorKind::NotADirectory,
        },
        Note::Publishing {
            topic: "lamp/health",
            unpublished: 7,
        },
        Note::BoardUnread {
            what: "cpu_load (/proc/loadavg: No such file or directory (os error 2))".into(),
        },
        Note::BoardRead,
        Note::Unreadable,
        Note::Readable { passed_over: 6 },
        Note::NotReceiving {
            kind: io::ErrorKind::PermissionDenied,
        },
        Note::Receiving,
        Note::TooManyParts,
        Note::NoSelfTest {
            kind: io::ErrorKind::PermissionDenied,
        },
        Note::SelfTestUnreadable,
    ]
    .iter()
    .map(Note::to_string)
    .collect();
    assert_eq!(
        lines,
        [
            "health ok",
            "health ok -> warn: board: cpul_thermal_zone is at 66 °C: the board slows itself down from 70",
            "health warn -> ok",
            "health stale: voice: has never reported",
            "lamp/health cannot be published (not a directory): it is tried again each second",
            "lamp/health is published again; unpublished=7",
            "the board cannot be read: cpu_load (/proc/loadavg: No such file or directory (os error 2))",
            "the board is read again",
            "a message on diagnostics is not a report: such messages are passed over",
            "diagnostics carries nothing but reports again; passed_over=6",
            "diagnostics cannot be received from (permission denied)",
            "diagnostics is received from again",
            "more than 64 parts report: the newcomers are not kept",
            "lamp/selftest cannot be received from (permission denied): the health goes on without it",
            "a message on lamp/selftest is not a whole run: it is passed over",
        ]
    );
    assert!(lines.iter().all(|line| !line.contains('\n')));
}

#[test]
fn the_lamps_own_times_follow_from_the_stale_time() {
    // As the topics promise: the board's report every second, the two
    // kept values every five.
    let times = Times::with_stale_after(serve::DEFAULT_STALE_AFTER);
    assert_eq!(
        (times.report_every, times.health_every, times.board_every),
        (
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(5)
        )
    );
    assert_eq!(serve::DEFAULT_STALE_AFTER, Duration::from_secs(5));
    assert_eq!(Diagnostic::declaration().period(), Some(times.report_every));
    assert_eq!(Health::declaration().period(), Some(times.health_every));
    assert_eq!(BoardState::declaration().period(), Some(times.board_every));
    // A longer stale time changes nothing; a short one has the board
    // report three times within it.
    assert_eq!(Times::with_stale_after(NEVER), times);
    assert_eq!(Times::with_stale_after(ms(3_000)), times);
    assert_eq!(Times::with_stale_after(ms(600)).report_every, ms(200));
    assert_eq!(Times::with_stale_after(ms(100)).report_every, ms(100) / 3);
}

#[test]
fn lamp_online_follows_the_voice_link_only_when_asked() {
    // Off: nothing is published on the topic.
    let mut quiet = Rig::new("offline-off");
    let mut voice = quiet.part("voice");
    report(&mut voice, Level::Ok, "voice", "");
    quiet.health_where("the voice is in", |health| {
        level_of(health, "voice").is_some()
    });
    assert!(
        quiet
            .bus
            .latest(lamp_msgs::Online::TOPIC)
            .unwrap()
            .is_none()
    );
    quiet.stop();
    // On: the voice's link, as it says it.
    let mut rig = Rig::with("offline-on", |options| options.online = true, None, |_| {});
    let mut voice = rig.part("voice");
    let mut online = rig
        .bus
        .subscribe_typed(lamp_msgs::Online::TOPIC, "test", lamp_msgs::Online::TYPE)
        .unwrap();
    for (link, wanted) in [("ready", Some(true)), ("down", Some(false))] {
        let sent = voice
            .publish_json(
                0,
                &Diagnostic::new(Level::Warn, "voice", "it cannot reach the speech service")
                    .with("link", link)
                    .with("down_for_s", 7),
            )
            .unwrap();
        assert!(sent.delivered >= 1);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert!(std::time::Instant::now() < deadline, "{link}: never said");
            let Some(message) = online.recv(ms(50)).unwrap() else {
                continue;
            };
            let said = message.parse::<lamp_msgs::Online>().unwrap();
            if said.online == wanted {
                assert!(said.check().is_ok());
                assert_eq!(said.from, "voice");
                assert_eq!(said.down_for_s, (wanted == Some(false)).then_some(7));
                break;
            }
        }
    }
    rig.stop();
}

/// A self-test run of these parts, begun now.
fn selftest(parts: &[(&str, Verdict, &str)]) -> SelfTest {
    let parts: Vec<PartResult> = parts
        .iter()
        .map(|(part, verdict, reason)| PartResult {
            part: (*part).into(),
            verdict: *verdict,
            reason: (*reason).into(),
            values: Vec::new(),
            took: 0.1,
        })
        .collect();
    SelfTest {
        occasion: Occasion::OnDemand,
        started: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64(),
        took: 2.0,
        verdict: SelfTest::verdict_of(&parts),
        parts,
        build: None,
    }
}

/// #812: a lamp says a part has failed before its person finds out. A run
/// that failed a part, kept on `lamp/selftest` before the loop began, makes
/// the lamp `error`, naming the part and why; a message that is no whole run
/// is passed over and said once; a run that could not check that part
/// leaves it failed; the next run, where it passed, makes the lamp well
/// again.
#[test]
fn a_part_the_selftest_failed_is_in_the_lamps_health_until_a_run_passes() {
    let mut failed = selftest(&[
        ("voice_mic", Verdict::Pass, ""),
        (
            "storage",
            Verdict::Fail,
            "1 I/O error on the card since boot",
        ),
        ("touch", Verdict::NotChecked, "Controls is not running"),
    ]);
    // A minute old, so that a later run is told apart by its age.
    failed.started -= 60.0;
    let mut rig = Rig::with(
        "selftest",
        |_| {},
        None,
        |rig| {
            let mut out = rig
                .bus
                .publisher_declared(SelfTest::TOPIC, &SelfTest::declaration(), "selftest")
                .unwrap();
            out.publish_json(0, &failed).unwrap();
        },
    );
    let health = rig.health_where("the failed run is in it", |health| {
        item(health, "selftest").is_some()
    });
    assert_eq!(
        item(&health, "selftest").map(|item| (item.level, item.message.as_str())),
        Some((
            Level::Error,
            "1 part failing: the SD card (1 I/O error on the card since boot)"
        ))
    );
    assert_eq!(
        (health.level, health.message.as_str()),
        (
            Level::Error,
            "selftest: 1 part failing: the SD card (1 I/O error on the card since boot)"
        )
    );

    let mut out = rig
        .bus
        .publisher_declared(SelfTest::TOPIC, &SelfTest::declaration(), "selftest")
        .unwrap();
    out.publish_json(1, &serde_json::json!({"not": "a run"}))
        .unwrap();
    rig.said("the message that is no run", |note| {
        *note == Note::SelfTestUnreadable
    });
    // A run that could not check storage leaves its fail standing (the
    // reviewer on #1243): the lamp is still error, now that run's age.
    let unchecked = selftest(&[
        ("voice_mic", Verdict::Pass, ""),
        (
            "storage",
            Verdict::NotChecked,
            "the kernel's log cannot be read",
        ),
    ]);
    out.publish_json(2, &unchecked).unwrap();
    let health = rig.health_where("the run that could not check storage is in it", |health| {
        item(health, "selftest").is_some_and(|item| item.age_ms < 1_000)
    });
    assert_eq!(
        (health.level, health.message.as_str()),
        (
            Level::Error,
            "selftest: 1 part failing: the SD card (1 I/O error on the card since boot)"
        )
    );
    let passed = selftest(&[
        ("voice_mic", Verdict::Pass, ""),
        ("storage", Verdict::Pass, ""),
        ("touch", Verdict::NotChecked, "Controls is not running"),
    ]);
    out.publish_json(3, &passed).unwrap();
    let health = rig.health_where("the passing run is in it", |health| {
        level_of(health, "selftest") == Some(Level::Ok)
    });
    assert_eq!(
        item(&health, "selftest").map(|item| item.message.as_str()),
        Some("passed; 1 not checked")
    );
    assert_eq!((health.level, health.message.as_str()), (Level::Ok, ""));
    rig.stop();
}

/// Says the voice's `link`, then waits for `lamp/online` to say `wanted`,
/// from `from`.
fn link_then_online(
    voice: &mut Publisher,
    online: &mut Subscriber,
    link: &str,
    wanted: Option<bool>,
    from: &str,
) {
    let sent = voice
        .publish_json(
            0,
            &Diagnostic::new(Level::Warn, "voice", "it cannot reach the speech service")
                .with("link", link)
                .with("down_for_s", 7),
        )
        .unwrap();
    assert!(sent.delivered >= 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "{link}: never said {wanted:?} from {from}"
        );
        let Some(message) = online.recv(ms(50)).unwrap() else {
            continue;
        };
        let said = message.parse::<lamp_msgs::Online>().unwrap();
        if said.online == wanted && said.from == from {
            assert!(said.check().is_ok());
            return;
        }
    }
}

#[test]
fn lamp_online_asks_the_internet_only_while_the_voice_link_is_down() {
    // A local listener stands for the internet (#1247, the reviewer).
    let internet = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    internet.set_nonblocking(true).unwrap();
    let target = internet.local_addr().unwrap();
    let mut rig = Rig::with(
        "reach-up",
        |options| {
            options.online = true;
            options.reach = Some(vec![target]);
        },
        None,
        |_| {},
    );
    let mut voice = rig.part("voice");
    let mut online = rig
        .bus
        .subscribe_typed(lamp_msgs::Online::TOPIC, "test", lamp_msgs::Online::TYPE)
        .unwrap();
    // The link up: online from the voice, and the internet is never asked.
    link_then_online(&mut voice, &mut online, "ready", Some(true), "voice");
    thread::sleep(ms(300));
    assert_eq!(
        internet.accept().map(|_| ()).unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "asked with the link up"
    );
    // The link down with the internet answering: online, from the probe.
    link_then_online(&mut voice, &mut online, "down", Some(true), "reach");
    assert!(internet.accept().is_ok(), "the probe connected");
    rig.stop();

    // The internet not answering: port 1, which nothing listens on and the
    // kernel refuses at once (El on #1247: a port just freed could be taken).
    let closed: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let mut rig = Rig::with(
        "reach-down",
        |options| {
            options.online = true;
            options.reach = Some(vec![closed]);
        },
        None,
        |_| {},
    );
    let mut voice = rig.part("voice");
    let mut online = rig
        .bus
        .subscribe_typed(lamp_msgs::Online::TOPIC, "test", lamp_msgs::Online::TYPE)
        .unwrap();
    link_then_online(&mut voice, &mut online, "down", Some(false), "reach");
    rig.stop();
}
