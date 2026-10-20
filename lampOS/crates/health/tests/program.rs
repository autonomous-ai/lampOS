//! The built program, started as a unit would start it: `lamp-health
//! serve` on a bus of the test's own, reading a board made of files, with
//! the test playing the lamp's parts; and `lamp-health` run against it as
//! a person would run it, for what it prints and the status it ends with.
mod tree;

use lamp_bus::{Bus, Kind, Notice, Publisher};
use lamp_health::show;
use lamp_msgs::{BoardState, Diagnostic, Health, HealthItem, Level};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tree::Tree;

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-health");
/// How long a test waits for something that is to happen. The machine
/// these run on is often busy.
const WAIT: Duration = Duration::from_secs(30);
const LOG_WAIT: Duration = Duration::from_secs(5);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// `serve`, killed when the test ends however it ends.
struct Running(Option<Child>);

/// A prefix in a partial pipe read is not yet a complete journal record.
fn printed_every_line(bytes: &[u8], wanted: &[&str]) -> bool {
    wanted.iter().all(|prefix| {
        bytes
            .split_inclusive(|byte| *byte == b'\n')
            .any(|line| line.ends_with(b"\n") && line.starts_with(prefix.as_bytes()))
    })
}

#[test]
fn journal_completion_waits_for_the_whole_terminal_record() {
    let wanted = ["lamp-health: started", "lamp-health: health warn -> error"];
    let partial = b"lamp-health: started\nlamp-health: health warn -> error";
    assert!(!printed_every_line(partial, &wanted));
    let mut complete = partial.to_vec();
    complete.extend_from_slice(b": voice: the link to the service is down\n");
    assert!(printed_every_line(&complete, &wanted));
    assert!(!printed_every_line(
        b"lamp-health: health warn -> error\n",
        &wanted
    ));
}

impl Running {
    /// Observe the final journal records before SIGKILL. Publication on the
    /// bus does not prove the child's log relay has reached its stderr pipe.
    fn end(mut self, last: &[&str]) -> Output {
        let mut stderr = self.0.as_mut().unwrap().stderr.take().unwrap();
        let (sent, printed) = mpsc::channel();
        let reader = thread::spawn(move || -> io::Result<()> {
            let mut chunk = [0u8; 4096];
            loop {
                let read = stderr.read(&mut chunk)?;
                if read == 0 || sent.send(chunk[..read].to_vec()).is_err() {
                    return Ok(());
                }
            }
        });
        let mut all = Vec::new();
        let since = Instant::now();
        let ready = loop {
            if printed_every_line(&all, last) {
                break true;
            }
            let left = LOG_WAIT.saturating_sub(since.elapsed());
            if left.is_zero() {
                break false;
            }
            match printed.recv_timeout(left) {
                Ok(bytes) => all.extend(bytes),
                Err(_) => break false,
            }
        };
        // Reap and drain even on a failed observation. Keep every byte for
        // the existing whole-journal assertions, including unexpected lines.
        let killed = self.0.as_mut().unwrap().kill();
        let mut output = self.0.take().unwrap().wait_with_output().unwrap();
        reader.join().unwrap().unwrap();
        all.extend(printed.try_iter().flatten());
        output.stderr = all;
        assert!(
            ready,
            "journal did not finish {last:?} within {LOG_WAIT:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        killed.unwrap();
        output
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A bus and a board of the test's own, removed when it ends.
struct Places {
    /// The bus: short, because a socket's whole path has to fit in 103
    /// bytes and the system's temporary directory on a Mac is too long.
    root: PathBuf,
    /// The real board, idle, as files.
    tree: Tree,
    bus: Bus,
}

impl Places {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(format!("/tmp/lhp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self {
            // Publishers look for new subscribers every 20 ms, not 200.
            bus: Bus::new(&root).unwrap().rescan_every(ms(20)),
            tree: Tree::real_board(&format!("program-{name}")),
            root,
        }
    }

    /// Start `serve` on this bus and board, and wait until it listens.
    fn serve(&self, more: &[&str]) -> Running {
        let places = self.tree.places();
        let child = Command::new(PROGRAM)
            .arg("serve")
            .arg("--bus-root")
            .arg(&self.root)
            .arg("--proc")
            .arg(&places.proc)
            .arg("--sys")
            .arg(&places.sys)
            .arg("--run")
            .arg(&places.run)
            .arg("--disk")
            .arg(&places.disk)
            .args(more)
            // Whatever the test was started with, the option decides.
            .env_remove("LAMP_BUS_ROOT")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let running = Running(Some(child));
        let started = Instant::now();
        while !self
            .bus
            .subscribers(Diagnostic::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.listening && listener.socket.starts_with("health."))
        {
            assert!(started.elapsed() < WAIT, "serve does not listen");
            thread::sleep(ms(10));
        }
        running
    }

    /// `lamp-health` with `arguments`, on this bus.
    fn show(&self, arguments: &[&str]) -> Output {
        Command::new(PROGRAM)
            .args(arguments)
            .arg("--bus-root")
            .arg(&self.root)
            .env_remove("LAMP_BUS_ROOT")
            .output()
            .unwrap()
    }

    /// The health the bus keeps, once it is one for which `wanted` holds.
    fn kept(&self, what: &str, wanted: impl Fn(&Health) -> bool) -> Health {
        let started = Instant::now();
        loop {
            let kept = self.bus.latest(Health::TOPIC).unwrap();
            let health = kept.map(|message| message.parse::<Health>().unwrap());
            if let Some(health) = health.as_ref().filter(|health| wanted(health)) {
                assert_eq!(health.check(), Ok(()));
                return health.clone();
            }
            assert!(
                started.elapsed() < WAIT,
                "no kept health where {what}; the last was {health:?}"
            );
            thread::sleep(ms(10));
        }
    }

    /// A part of the lamp, as the parts publish: the topic declared.
    fn part(&self, name: &str) -> Publisher {
        self.bus
            .publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), name)
            .unwrap()
    }
}

impl Drop for Places {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

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

fn item<'a>(health: &'a Health, name: &str) -> Option<&'a HealthItem> {
    health.items.iter().find(|item| item.name == name)
}

fn printed(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// What `show` printed and the status it ended with.
fn shown(output: &Output) -> (String, Option<i32>) {
    (printed(&output.stdout), output.status.code())
}

/// The status `show` ends with for a lamp whose parts the test made as
/// bad as `parts`, with the board as it is.
///
/// The board's level is not the test's to choose: the disk it reports on
/// is the machine's own. With a tenth of it free the board is ok and this
/// is the status of `parts`. On a machine whose disk is nearly full the
/// board has a warning or an error of its own, and that counts too.
fn status(parts: Level, board: Level) -> Option<i32> {
    let worst = if board.rank() > parts.rank() {
        board
    } else {
        parts
    };
    Some(match worst {
        Level::Ok => 0,
        Level::Warn => 1,
        Level::Error => 2,
        Level::Stale => 3,
    })
}

/// The board's level in the health the bus keeps, and a line for whoever
/// reads the test's output if it is not ok: what the test then asserts of
/// the words `show` prints is less.
fn board_level(health: &Health) -> Level {
    let board = item(health, "board").expect("the board is in the health");
    if board.level != Level::Ok {
        eprintln!(
            "the board is not ok on this machine ({}): the test checks statuses and not every line",
            board.message
        );
    }
    board.level
}

#[test]
fn show_says_how_the_lamp_is_and_ends_with_its_level() {
    let places = Places::new("levels");
    // Nothing goes stale in this test, however slow the machine.
    let running = places.serve(&["--stale-ms", "600000"]);
    let alone = places.kept("the board is in it", |health| {
        item(health, "board").is_some()
    });
    // The board is as well as the machine's own disk lets it be.
    let board = board_level(&alone);
    let well = board == Level::Ok;

    // The board alone.
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, status(Level::Ok, board), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    // The footer says how old the picture is only once it is over 5 s old.
    assert!(
        lines.len() == 5 || (lines.len() == 6 && lines[5].starts_with("written ")),
        "{out}"
    );
    if well {
        assert_eq!(lines[0], "The lamp is well.");
        assert_eq!(lines[2], "  ok     computer", "{out}");
    }
    assert_eq!(lines[1], "");
    assert_eq!(lines[3], "");
    // The hottest of the eight zones, 64.5 °C, and the other numbers.
    assert!(
        lines[4].starts_with("computer  hottest 64 °C  memory 44% used  disk "),
        "{out}"
    );
    assert!(lines[4].ends_with("% free  up 2 d 10 h"), "{out}");
    // `show` is what no command means.
    let (same, ended) = shown(&places.show(&["show"]));
    assert_eq!(ended, status(Level::Ok, board));
    assert_eq!(same.lines().next(), lines.first().copied());

    // Two parts report that they are well.
    let (mut voice, mut body) = (places.part("voice"), places.part("body"));
    report(&mut voice, Level::Ok, "voice", "");
    report(&mut body, Level::Ok, "body", "");
    places.kept("both parts are in it", |health| health.items.len() == 3);
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, status(Level::Ok, board), "{out}");
    if well {
        // Parts as well as each other come by the name shown.
        assert!(
            out.starts_with(
                "The lamp is well.\n\n  ok     arm and head\n  ok     computer\n  ok     voice\n"
            ),
            "{out}"
        );
    }
    assert!(out.contains("\n  ok     arm and head\n"), "{out}");
    assert!(out.contains("\n  ok     voice\n"), "{out}");

    // As JSON: the two values as they were published, on one line.
    let output = places.show(&["--json"]);
    assert_eq!(output.status.code(), status(Level::Ok, board));
    let out = printed(&output.stdout);
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.starts_with("{\"health\":{\"level\":\""), "{out}");
    let both: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(both.as_object().unwrap().len(), 2);
    let health: Health = serde_json::from_value(both["health"].clone()).unwrap();
    assert_eq!(health.check(), Ok(()));
    let mut names: Vec<&str> = health.items.iter().map(|item| item.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["board", "body", "voice"]);
    assert_eq!(item(&health, "voice").unwrap().level, Level::Ok);
    let state: BoardState = serde_json::from_value(both["board"].clone()).unwrap();
    assert_eq!(state.check(), Ok(()));
    assert_eq!(state.temperature_c.len(), 8);
    assert_eq!(state.temperature_c["ddr_thermal_zone"], 64.5);
    assert_eq!(state.uptime_s, 208_800);
    assert!(output.stderr.is_empty());

    // The body has a warning.
    report(&mut body, Level::Warn, "body", "yaw is hot");
    places.kept("the body's warning is in it", |health| {
        item(health, "body").is_some_and(|body| body.level == Level::Warn)
    });
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, status(Level::Warn, board), "{out}");
    if well {
        assert!(
            out.starts_with(
                "The lamp has a warning: arm and head: yaw is hot\n\n  warn   arm and head  yaw is hot\n"
            ),
            "{out}"
        );
    }
    let output = places.show(&["--json"]);
    assert_eq!(output.status.code(), status(Level::Warn, board));
    let both: Value = serde_json::from_slice(&output.stdout).unwrap();
    if well {
        assert_eq!(both["health"]["level"], "warn");
        assert_eq!(both["health"]["message"], "body: yaw is hot");
    }

    // The voice has an error, which is worse.
    report(
        &mut voice,
        Level::Error,
        "voice",
        "the link to the service is down",
    );
    places.kept("the voice's error is in it", |health| {
        item(health, "voice").is_some_and(|voice| voice.level == Level::Error)
    });
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, Some(2), "{out}");
    if well {
        assert!(
            out.starts_with(
                "The lamp is not well: voice: the link to the service is down\n\n\
                 \x20 error  voice         the link to the service is down\n"
            ),
            "{out}"
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[3] == "  warn   arm and head  yaw is hot", "{out}");
        assert!(lines[4] == "  ok     computer", "{out}");
    }
    assert_eq!(places.show(&["--json"]).status.code(), Some(2));

    // What `serve` printed while all this went on: how it started, and
    // the lamp's level each time it became another. No line for any of
    // the reports.
    // An already-error board never changes level. Otherwise the final
    // voice error follows the warning, even when that warning is the disk's.
    let last = if board == Level::Error {
        "lamp-health: health error"
    } else {
        "lamp-health: health warn -> error"
    };
    let output = running.end(&["lamp-health: started bus_root=", last]);
    assert!(output.stdout.is_empty());
    let journal = printed(&output.stderr);
    let from = places.tree.places();
    let lines: Vec<&str> = journal.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "lamp-health: started bus_root={} expect=none stale_ms=600000 proc={} sys={} run={} disk={}",
            places.root.display(),
            from.proc.display(),
            from.sys.display(),
            from.run.display(),
            from.disk.display()
        )
    );
    if well {
        assert_eq!(
            lines[1..],
            [
                "lamp-health: health ok",
                "lamp-health: health ok -> warn: body: yaw is hot",
                "lamp-health: health warn -> error: voice: the link to the service is down",
            ],
            "{journal}"
        );
    }
    assert!(
        lines.iter().all(|line| line.starts_with("lamp-health: ")),
        "{journal}"
    );
}

#[test]
fn with_nobody_serving_show_says_so_and_ends_with_3() {
    let places = Places::new("nobody");
    let nobody = format!("{}\nLooked in {}.\n", show::NOBODY, places.root.display());
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!((out.as_str(), ended), (nobody.as_str(), Some(3)));
    let (out, ended) = shown(&places.show(&["--json"]));
    assert_eq!(
        (out.as_str(), ended),
        ("{\"health\":null,\"board\":null}\n", Some(3))
    );
    // The bus is found by `LAMP_BUS_ROOT` when no option names it, and a
    // bus that is not there at all is the same as an empty one.
    let elsewhere = places.root.join("not-made");
    let output = Command::new(PROGRAM)
        .env("LAMP_BUS_ROOT", &elsewhere)
        .output()
        .unwrap();
    assert_eq!(
        shown(&output),
        (
            format!("{}\nLooked in {}.\n", show::NOBODY, elsewhere.display()),
            Some(3)
        )
    );
    assert!(output.stderr.is_empty());
    assert!(!elsewhere.exists(), "show made something on the bus");

    // Something on the topic that is no health: said, and still 3.
    let mut odd = places
        .bus
        .publisher(Health::TOPIC, Kind::Latest, "odd")
        .unwrap();
    odd.publish_json(0, &json!({"level": "splendid"})).unwrap();
    let output = places.show(&[]);
    assert_eq!(shown(&output), (nobody.clone(), Some(3)));
    let said = printed(&output.stderr);
    assert!(
        said.starts_with("lamp-health: lamp/health holds something else: "),
        "{said}"
    );
    // A health whose level is not its parts' worst is not believed.
    odd.publish_json(
        0,
        &json!({"level": "ok", "message": "", "missing": [], "items": [
            {"name": "voice", "level": "error", "message": "down", "age_ms": 1}
        ]}),
    )
    .unwrap();
    let output = places.show(&["--json"]);
    assert_eq!(
        shown(&output),
        ("{\"health\":null,\"board\":null}\n".to_owned(), Some(3))
    );
    assert!(
        printed(&output.stderr).starts_with("lamp-health: lamp/health holds what cannot be: "),
        "{}",
        printed(&output.stderr)
    );
    drop(odd);

    // A health that was written and is still within its time is shown,
    // though whoever wrote it has gone: `show` reads what the bus keeps.
    let well = Health {
        level: Level::Ok,
        message: String::new(),
        items: vec![HealthItem {
            name: "board".into(),
            level: Level::Ok,
            message: String::new(),
            age_ms: 120,
        }],
        missing: Vec::new(),
        quiet_ms: 0,
    };
    let mut health = places
        .bus
        .publisher_declared(Health::TOPIC, &Health::declaration(), "health")
        .unwrap();
    health.publish_json(0, &well).unwrap();
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, Some(0), "{out}");
    assert!(
        out.starts_with(
            "The lamp is well.\n\n  ok     computer\n\n\
             computer  no numbers: nobody is publishing board/state"
        ),
        "{out}"
    );
    // One that has outlived its time is the same as none.
    health.expires_after(Some(ms(1)));
    health.publish_json(0, &well).unwrap();
    thread::sleep(ms(200));
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!((out.as_str(), ended), (nobody.as_str(), Some(3)));
}

#[test]
fn a_part_that_is_expected_and_silent_or_has_gone_quiet_ends_show_with_3() {
    let places = Places::new("stale");
    let running = places.serve(&[
        "--expect",
        "eyes",
        "--expect",
        "voice",
        "--stale-ms",
        "1500",
    ]);
    // The voice reports once and then no more. The eyes never do.
    let mut voice = places.part("voice");
    report(&mut voice, Level::Ok, "voice", "");
    let heard = places.kept("the voice has reported", |health| {
        item(health, "voice").is_some()
    });
    let board = board_level(&heard);
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, status(Level::Stale, board), "{out}");
    if board == Level::Ok {
        assert!(
            out.starts_with("Not all of the lamp is reporting: "),
            "{out}"
        );
    }
    assert!(out.contains("\n  not running: camera\n"), "{out}");

    let quiet = places.kept("the voice is stale", |health| {
        item(health, "voice").is_some_and(|voice| voice.level == Level::Stale)
    });
    assert_eq!(quiet.missing, ["eyes"]);
    let (out, ended) = shown(&places.show(&[]));
    assert_eq!(ended, status(Level::Stale, board), "{out}");
    assert!(
        out.contains("\n  quiet  voice     not heard from for "),
        "{out}"
    );
    assert!(out.contains("\n  not running: camera\n"), "{out}");
    let output = places.show(&["--json"]);
    assert_eq!(output.status.code(), status(Level::Stale, board));
    let both: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(both["health"]["missing"], json!(["eyes"]));
    assert!(both["board"].is_object());

    let journal = printed(
        &running
            .end(&["lamp-health: started bus_root=", "lamp-health: health "])
            .stderr,
    );
    let lines: Vec<&str> = journal.lines().collect();
    assert!(
        lines[0].starts_with("lamp-health: started bus_root=")
            && lines[0].contains(" expect=eyes,voice stale_ms=1500 proc="),
        "{journal}"
    );
    // Before anything had reported, both were missing: the first named.
    // On a machine whose own board isn't well (a disk under 10 % free),
    // the board's line comes first instead: as `board_level` says, the
    // test then checks statuses and not every line.
    if board == Level::Ok {
        assert_eq!(
            lines[1],
            "lamp-health: health stale: eyes: has never reported"
        );
    }
}

#[test]
fn between_two_healths_five_seconds_apart_the_program_is_not_taken_for_gone() {
    let places = Places::new("lease");
    // A bus as the lamp's programs use it: a lease of three seconds, and
    // subscribers who ask now and then what has become of their topics.
    let bus = Bus::new(&places.root).unwrap();
    let mut health = bus
        .subscribe_typed(Health::TOPIC, "test", Health::TYPE)
        .unwrap();
    let mut board = bus
        .subscribe_typed(BoardState::TOPIC, "test", BoardState::TYPE)
        .unwrap();
    // The program as it runs on the lamp: every time its own.
    let running = places.serve(&[]);
    let first = health.recv(WAIT).unwrap().expect("a first health");
    board.recv(WAIT).unwrap().expect("the board's numbers");
    // Until the health is published a second time, with nothing new to
    // say, neither topic's source is ever gone and neither is late.
    let started = Instant::now();
    let mut noticed: Vec<(&str, Notice)> = Vec::new();
    let second = loop {
        noticed.extend(
            health
                .notices()
                .into_iter()
                .map(|notice| ("health", notice)),
        );
        noticed.extend(board.notices().into_iter().map(|notice| ("board", notice)));
        if let Some(message) = health.recv(ms(100)).unwrap() {
            break message;
        }
        assert!(started.elapsed() < WAIT, "no second health in {WAIT:?}");
    };
    noticed.extend(
        health
            .notices()
            .into_iter()
            .map(|notice| ("health", notice)),
    );
    noticed.extend(board.notices().into_iter().map(|notice| ("board", notice)));
    assert!(noticed.is_empty(), "{noticed:?}");
    assert_eq!((first.header.seq, second.header.seq), (1, 2));
    assert_eq!(second.gap, 0);
    // Five seconds after the first by the program's own clock, give or
    // take what a busy machine holds it up by: under the ten at which the
    // topic would be late, and not with every report either.
    let apart = second.header.at_us - first.header.at_us;
    assert!((4_000_000..10_000_000).contains(&apart), "{apart} us");
    let (one, two): (Health, Health) = (first.parse().unwrap(), second.parse().unwrap());
    assert_eq!(one.check().and(two.check()), Ok(()));
    assert_eq!(
        (one.level, &one.message, one.items.len()),
        (two.level, &two.message, two.items.len())
    );
    // Its board's numbers came a second time as well by now, or will.
    board
        .recv(WAIT)
        .unwrap()
        .expect("the board's numbers again");
    let journal = printed(
        &running
            .end(&["lamp-health: started bus_root=", "lamp-health: health "])
            .stderr,
    );
    assert!(
        journal.contains(" expect=none stale_ms=5000 proc="),
        "{journal}"
    );
    assert_eq!(journal.lines().count(), 2, "{journal}");
}

#[test]
fn wrong_arguments_print_the_usage_and_end_with_2_for_serve_and_64_for_show() {
    let run = |arguments: &[&str]| {
        Command::new(PROGRAM)
            .args(arguments)
            .env_remove("LAMP_BUS_ROOT")
            .output()
            .unwrap()
    };
    let usage = "\nusage: lamp-health serve [--bus-root ABSOLUTE_DIR] [--expect NAME]... [--stale-ms N] [--judge-zone BEGINNING]... [--proc DIR] [--sys DIR] [--run DIR] [--disk PATH] [--clock timesync|kernel] [--online on|off] [--reach-probe on|off] [--needs NAME=PATH]...\n       lamp-health [show] [--bus-root ABSOLUTE_DIR] [--json]\n       lamp-health --help\n";
    for (arguments, ends) in [
        (&["serve", "--bus-root", "relative"][..], 2),
        (&["serve", "--stale-ms", "99"], 2),
        (&["serve", "--stale-ms", "600001"], 2),
        (&["serve", "--proc", "proc"], 2),
        (&["serve", "--expect", "voice", "--expect", "voice"], 2),
        (&["serve", "--expect", "vo\nice"], 2),
        (&["serve", "--loud", "yes"], 2),
        (&["serve", "--json"], 2),
        (&["serve", "--reach-probe", "on"], 2),
        (&["show", "--loud"], 64),
        (&["show", "--bus-root", "relative"], 64),
        (&["show", "--bus-root"], 64),
        (&["show", "--expect", "voice"], 64),
        (&["--stale-ms", "5000"], 64),
        (&["--json", "--json"], 64),
        (&["status"], 64),
    ] {
        let output = run(arguments);
        assert_eq!(output.status.code(), Some(ends), "{arguments:?}");
        let said = printed(&output.stderr);
        assert!(said.starts_with("lamp-health: "), "{said}");
        assert!(said.ends_with(usage), "{arguments:?}: {said}");
        assert!(output.stdout.is_empty());
    }
    let unknown = printed(&run(&["serve", "--loud", "yes"]).stderr);
    assert!(
        unknown.starts_with("lamp-health: unknown option --loud\nusage: "),
        "{unknown}"
    );

    for arguments in [
        &["--help"][..],
        &["-h"],
        &["serve", "--help"],
        &["show", "-h"],
    ] {
        let help = run(arguments);
        assert_eq!(help.status.code(), Some(0), "{arguments:?}");
        let out = printed(&help.stdout);
        assert!(out.starts_with(&usage[1..]), "{out}");
        // It says what each status means, and which one is a mistake.
        for said in [
            "\n  --stale-ms  after how long",
            "\n  --expect    a part",
            "\n  --json      the two values",
            "status 0 if the lamp is ok, 1 for a warning, 2 for an error, and 3 if",
            "status 64",
            "status 2.",
        ] {
            assert!(out.contains(said), "{said:?} is not in {out}");
        }
        assert!(help.stderr.is_empty());
    }

    // A bus named wrongly by the environment: a mistake in asking, for
    // `show`, and for `serve` an end before anything is listened to.
    let with_root = |arguments: &[&str]| {
        Command::new(PROGRAM)
            .args(arguments)
            .env("LAMP_BUS_ROOT", "relative/bus")
            .output()
            .unwrap()
    };
    let output = with_root(&[]);
    assert_eq!(output.status.code(), Some(64));
    assert!(
        printed(&output.stderr)
            .starts_with("lamp-health: bus root relative/bus is not an absolute path\nusage: "),
        "{}",
        printed(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let output = with_root(&["serve"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        printed(&output.stderr),
        "lamp-health: bus root relative/bus is not an absolute path\n"
    );
}

#[test]
fn a_bus_that_cannot_be_subscribed_to_ends_serve_before_it_listens() {
    // A file where the bus's directory would be.
    let file = PathBuf::from(format!("/tmp/lhp-file-{}", std::process::id()));
    let _ = fs::remove_dir_all(&file);
    let _ = fs::remove_file(&file);
    fs::write(&file, b"in the way").unwrap();
    let output = Command::new(PROGRAM)
        .arg("serve")
        .arg("--bus-root")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let said = printed(&output.stderr);
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.len(), 2, "{said}");
    assert!(
        lines[0].starts_with("lamp-health: started bus_root="),
        "{said}"
    );
    assert!(lines[1].starts_with("lamp-health: "), "{said}");
    assert!(output.stdout.is_empty());
    // And `show`, asked of the same place, says that nobody is there.
    let output = Command::new(PROGRAM)
        .arg("--bus-root")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(Path::new(&file).is_file());
    fs::remove_file(&file).unwrap();
}
