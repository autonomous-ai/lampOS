use super::*;
use serde_json::json;
use std::path::PathBuf;

const SECOND: u64 = 1_000_000;

/// The process's own watches are one: every test that takes or reads them
/// runs one at a time.
static ONE: Mutex<()> = Mutex::new(());

fn stuffy() -> Reading {
    Reading::Air {
        co2_ppm: Some(1_250),
    }
}

#[test]
fn a_watch_fires_once_and_is_over() {
    let mut watches = Watches::default();
    watches.add(Fact::Co2Above(1_200), 0, LONGEST_US).unwrap();
    // Below the level: nothing, and still watched.
    let told = watches.read(&Reading::Air { co2_ppm: Some(900) }, SECOND, true);
    assert!(told.is_empty() && watches.len() == 1);
    let told = watches.read(&stuffy(), 2 * SECOND, true);
    assert_eq!(told.len(), 1);
    assert!(
        told[0].contains("1250"),
        "the fact is in the note: {told:?}"
    );
    assert!(watches.is_empty());
    // Once: the next reading says nothing.
    assert!(watches.read(&stuffy(), 3 * SECOND, true).is_empty());
}

#[test]
fn no_reading_of_the_air_is_no_news() {
    let mut watches = Watches::default();
    watches.add(Fact::Co2Above(1_200), 0, LONGEST_US).unwrap();
    assert!(
        watches
            .read(&Reading::Air { co2_ppm: None }, SECOND, true)
            .is_empty()
    );
    assert_eq!(watches.len(), 1);
}

#[test]
fn at_most_five_at_once_and_a_day_each() {
    let mut watches = Watches::default();
    for _ in 0..MOST {
        watches.add(Fact::Arrives, 0, LONGEST_US * 3).unwrap();
    }
    assert_eq!(watches.add(Fact::Leaves, 0, LONGEST_US), Err(Refused::Full));
    // None lasts past a day, whatever was asked.
    assert_eq!(watches.expire(LONGEST_US - 1), 0);
    assert_eq!(watches.expire(LONGEST_US), MOST);
    assert!(watches.is_empty());
    // A watch that ended makes room.
    watches.add(Fact::Leaves, LONGEST_US, LONGEST_US).unwrap();
}

#[test]
fn a_watch_with_its_own_shorter_time_ends_at_it() {
    let mut watches = Watches::default();
    watches
        .add(Fact::Co2Above(1_200), 0, 3_600 * SECOND)
        .unwrap();
    // Past its hour, the reading that would have fired it does not.
    assert!(watches.read(&stuffy(), 3_600 * SECOND, true).is_empty());
    assert!(watches.is_empty());
}

#[test]
fn stop_watching_ends_every_watch() {
    let mut watches = Watches::default();
    watches.add(Fact::Arrives, 0, LONGEST_US).unwrap();
    watches
        .add(Fact::SittingOver(3_600), 0, LONGEST_US)
        .unwrap();
    assert_eq!(watches.stop_all(), 2);
    assert_eq!(watches.stop_all(), 0);
    assert!(
        watches
            .read(&Reading::Sitting { sitting_s: 7_200 }, SECOND, true)
            .is_empty()
    );
}

#[test]
fn a_person_arriving_is_told_once_from_counts_alone() {
    let mut watches = Watches::default();
    watches.add(Fact::Arrives, 0, LONGEST_US).unwrap();
    let people = |count| Reading::People {
        count,
        facing: false,
    };
    // Nobody: nothing. A first face: arrived, once; the watch is over.
    assert!(watches.read(&people(0), 10 * SECOND, true).is_empty());
    let told = watches.read(&people(1), 20 * SECOND, true);
    assert_eq!(told.len(), 1);
    assert!(!told[0].to_lowercase().contains("name"), "{told:?}");
    assert!(told[0].contains("Do not say who"), "{told:?}");
    assert!(watches.read(&people(2), 21 * SECOND, true).is_empty());
}

#[test]
fn leaving_needs_five_seconds_of_nobody_and_turning_needs_a_turn() {
    let mut watches = Watches::default();
    watches.add(Fact::Leaves, 0, LONGEST_US).unwrap();
    watches.add(Fact::TurnsToLamp, 0, LONGEST_US).unwrap();
    let people = |count, facing| Reading::People { count, facing };
    assert!(
        watches
            .read(&people(1, false), 10 * SECOND, true)
            .is_empty()
    );
    // Nobody for one look: not yet left.
    assert!(
        watches
            .read(&people(0, false), 12 * SECOND, true)
            .is_empty()
    );
    assert_eq!(watches.len(), 2);
    // Turned to the lamp: told, that watch over.
    let told = watches.read(&people(1, true), 13 * SECOND, true);
    assert_eq!(told.len(), 1);
    assert_eq!(watches.len(), 1);
    assert!(
        watches
            .read(&people(0, false), 14 * SECOND, true)
            .is_empty()
    );
    // Five seconds on from the last face seen: left.
    assert_eq!(watches.read(&people(0, false), 18 * SECOND, true).len(), 1);
    assert!(watches.is_empty());
}

#[test]
fn a_sitting_and_a_time_of_day_fire_at_their_mark() {
    let mut watches = Watches::default();
    watches
        .add(Fact::SittingOver(3_600), 0, LONGEST_US)
        .unwrap();
    watches.add(Fact::At(1_000_000), 0, LONGEST_US).unwrap();
    assert!(
        watches
            .read(&Reading::Sitting { sitting_s: 3_599 }, SECOND, true)
            .is_empty()
    );
    assert_eq!(
        watches
            .read(&Reading::Sitting { sitting_s: 3_600 }, 2 * SECOND, true)
            .len(),
        1
    );
    assert!(
        watches
            .read(&Reading::Clock { unix_ms: 999_999 }, 3 * SECOND, true)
            .is_empty()
    );
    assert_eq!(
        watches
            .read(&Reading::Clock { unix_ms: 1_000_000 }, 4 * SECOND, true)
            .len(),
        1
    );
    assert!(watches.is_empty());
}

#[test]
fn the_call_is_read_with_bounds() {
    let now = 1_700_000_000_000;
    let ok = |args: Value| parse(&args, now, Some(600));
    assert_eq!(ok(json!({"what": "arrives"})).unwrap().0, Fact::Arrives);
    assert_eq!(
        ok(json!({"what": "co2_above"})).unwrap().0,
        Fact::Co2Above(STUFFY_PPM)
    );
    assert_eq!(
        ok(json!({"what": "co2_above", "ppm": 1500})).unwrap().0,
        Fact::Co2Above(1_500)
    );
    assert_eq!(
        ok(json!({"what": "sitting_over", "minutes": 60}))
            .unwrap()
            .0,
        Fact::SittingOver(3_600)
    );
    assert_eq!(
        ok(json!({"what": "arrives", "for_hours": 2})).unwrap().1,
        2 * 3_600 * SECOND
    );
    for bad in [
        json!({}),
        json!({"what": "teleports"}),
        json!({"what": "co2_above", "ppm": 100}),
        json!({"what": "co2_above", "ppm": 9000}),
        json!({"what": "co2_above", "ppm": "high"}),
        json!({"what": "sitting_over"}),
        json!({"what": "sitting_over", "minutes": 0}),
        json!({"what": "arrives", "for_hours": 25}),
        json!({"what": "arrives", "for_hours": 0}),
        json!({"what": "at_time"}),
        json!({"what": "at_time", "time": "25:00"}),
        json!({"what": "at_time", "time": "noon"}),
    ] {
        assert!(ok(bad.clone()).is_err(), "{bad}");
    }
    // The clock: 10:00 local now (minute 600), at 10:30 is 30 minutes on;
    // at 10:00 is a day on (not now); at 09:59 is 23 h 59 min on.
    let at = |time: &str| match parse(&json!({"what": "at_time", "time": time}), now, Some(600))
        .unwrap()
        .0
    {
        Fact::At(ms) => (ms - (now - now % 60_000)) / 60_000,
        other => panic!("{other:?}"),
    };
    assert_eq!(at("10:30"), 30);
    assert_eq!(at("09:59"), 1_439);
    // This very minute is refused; a time farther than the watch lasts too.
    assert!(parse(&json!({"what": "at_time", "time": "10:00"}), now, Some(600)).is_err());
    assert!(
        parse(
            &json!({"what": "at_time", "time": "12:00", "for_hours": 1}),
            now,
            Some(600)
        )
        .is_err()
    );
    assert!(
        parse(
            &json!({"what": "at_time", "time": "10:30", "for_hours": 1}),
            now,
            Some(600)
        )
        .is_ok()
    );
    // No zone known: a time of day cannot be set.
    assert!(parse(&json!({"what": "at_time", "time": "10:30"}), now, None).is_err());
}

#[test]
fn the_tool_takes_five_and_stop_ends_them() {
    // The process's own watches: one test at a time uses them.
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    WATCHES.lock().unwrap().stop_all();
    for n in 0..MOST {
        let told = run(WATCH, &json!({"what": "arrives"}));
        assert!(told.1, "{n}: {told:?}");
    }
    let told = run(WATCH, &json!({"what": "leaves"}));
    assert!(!told.1);
    assert_eq!(told.0["error"], FULL);
    let told = run(STOP, &json!({}));
    assert_eq!(told.0["say"], STOPPED);
    assert_eq!(run(STOP, &json!({})).0["say"], NOTHING);
    assert!(WATCHES.lock().unwrap().is_empty());
}

#[test]
fn the_tools_are_declared_only_with_the_switch() {
    assert_eq!(arguments(Some("on")), vec![FLAG.to_owned()]);
    assert!(arguments(Some("1")).is_empty() && arguments(None).is_empty());
    let names: Vec<String> = tools().into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, [WATCH, STOP]);
    assert!(is_tool(WATCH) && is_tool(STOP) && !is_tool("desk_stop"));
}

#[test]
fn a_told_watch_is_a_note_the_say_service_would_take() {
    let line = note_line("The air is stuffy. Say it in one line.");
    let request = crate::say::parse(&line.to_string()).expect("a request the say service takes");
    assert_eq!(request.skill, SKILL);
    assert_eq!(request.priority, PRIORITY);
    assert_eq!(line["expires_in_ms"], EXPIRES_MS);
    // A note, never written words.
    assert!(matches!(request.words, crate::say::Words::Note(_)));
}

#[test]
fn the_say_service_not_being_there_is_said_and_nothing_hangs() {
    let gone = std::env::temp_dir().join(format!("lampos-watch-nosay-{}", std::process::id()));
    assert_eq!(
        say_at(&gone, &note_line("x")),
        "the say service is not there; not told"
    );
}

#[test]
fn while_the_lamp_may_not_speak_a_level_waits_and_an_edge_is_over() {
    let mut watches = Watches::default();
    watches.add(Fact::Co2Above(1_200), 0, LONGEST_US).unwrap();
    watches.add(Fact::Arrives, 0, LONGEST_US).unwrap();
    // The air is stuffy and someone comes in, but the lamp may not speak
    // (the private room, asleep): nothing is said.
    let told = watches.read(&stuffy(), SECOND, false);
    assert!(told.is_empty());
    // The room is known empty first (the first look is only a baseline).
    let nobody = Reading::People {
        count: 0,
        facing: false,
    };
    assert!(watches.read(&nobody, 5 * SECOND, false).is_empty());
    let told = watches.read(
        &Reading::People {
            count: 1,
            facing: false,
        },
        10 * SECOND,
        false,
    );
    assert!(told.is_empty());
    // The arrival is over for good (said to no one); the air still waits.
    assert_eq!(watches.len(), 1);
    // The lamp may speak again: the next reading of the air tells it.
    let told = watches.read(&stuffy(), 20 * SECOND, true);
    assert_eq!(told.len(), 1);
    assert!(watches.is_empty());
}

#[test]
fn the_private_room_and_a_missing_bus_gate_the_watcher() {
    assert!(may_speak(None, 0));
    crate::person::set_private_room_for_test(Some(true));
    assert!(!may_speak(None, 0));
    crate::person::set_private_room_for_test(None);
}

#[test]
fn someone_already_there_when_the_watch_is_taken_does_not_arrive() {
    let mut watches = Watches::default();
    let people = |count| Reading::People {
        count,
        facing: false,
    };
    // Seen with no watch: it is known they are there.
    assert!(watches.read(&people(1), 10 * SECOND, true).is_empty());
    watches.add(Fact::Arrives, 12 * SECOND, LONGEST_US).unwrap();
    assert!(watches.read(&people(1), 13 * SECOND, true).is_empty());
    // They leave and a person comes in later: that is an arrival.
    assert!(watches.read(&people(0), 14 * SECOND, true).is_empty());
    assert!(watches.read(&people(0), 20 * SECOND, true).is_empty());
    assert!(watches.read(&people(0), 30 * SECOND, true).is_empty());
    assert_eq!(watches.read(&people(1), 40 * SECOND, true).len(), 1);
}

#[test]
fn a_time_told_hours_late_is_dropped_not_said_as_now() {
    let mut watches = Watches::default();
    watches.add(Fact::At(1_000_000), 0, LONGEST_US).unwrap();
    // The lamp could not speak at the time; much later it may: dropped.
    let late = 1_000_000 + AT_LATE_MS + 1;
    assert!(
        watches
            .read(&Reading::Clock { unix_ms: late }, SECOND, true)
            .is_empty()
    );
    assert!(watches.is_empty());
    // A few minutes late is still told.
    let mut watches = Watches::default();
    watches.add(Fact::At(1_000_000), 0, LONGEST_US).unwrap();
    assert_eq!(
        watches
            .read(
                &Reading::Clock {
                    unix_ms: 1_000_000 + AT_LATE_MS
                },
                SECOND,
                true
            )
            .len(),
        1
    );
}

/// The whole path in one process, as the provider worker runs it: the tool
/// call takes the watch, the watcher thread reads the bus, and the say
/// service's socket is handed the note (Heron's P1 on #7782: the tool and the
/// watcher must be in the same process). The time from the reading on the bus
/// to the note on the socket is measured.
#[test]
fn a_tool_call_then_a_reading_on_the_bus_reaches_the_say_socket_in_one_process() {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixListener;
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    WATCHES.lock().unwrap().stop_all();
    let dir = std::path::PathBuf::from(format!("/tmp/vwatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(dir.join("bus")).expect("a bus");
    let mut air = bus
        .publisher_declared(AirState::TOPIC, &AirState::declaration(), "air")
        .expect("a publisher");
    let socket = dir.join("say.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (heard, got) = std::sync::mpsc::channel::<(String, std::time::Instant)>();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut line = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            if reader.read_line(&mut line).is_ok() {
                let _ = heard.send((line.clone(), std::time::Instant::now()));
                let mut stream = stream;
                let _ = writeln!(stream, "{{\"outcome\":\"spoken\"}}");
            }
        }
    });
    let _watcher = start_on(&bus, socket).expect("the watcher starts");
    // The tool call, as the route makes it.
    let told = run(WATCH, &json!({"what": "co2_above", "ppm": 1000}));
    assert!(told.1, "{told:?}");
    let reading = |ppm: u16| AirState {
        pm1_0: None,
        pm2_5: None,
        pm4_0: None,
        pm10: None,
        humidity: None,
        temperature: None,
        co2_ppm: Some(ppm),
        feel: lamp_msgs::AirFeel::Unknown,
    };
    air.publish_json(lamp_ipc::monotonic_us(), &reading(800))
        .unwrap();
    assert!(
        got.recv_timeout(Duration::from_millis(600)).is_err(),
        "below the level"
    );
    let published = std::time::Instant::now();
    air.publish_json(lamp_ipc::monotonic_us(), &reading(1_250))
        .unwrap();
    let (line, at) = got.recv_timeout(Duration::from_secs(2)).expect("the note");
    let took = at.duration_since(published);
    eprintln!("watch: a reading on the bus to the note on the say socket: {took:?}");
    assert!(took < Duration::from_secs(1), "{took:?}");
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["skill"], SKILL);
    assert!(request["note"].as_str().unwrap().contains("1250"), "{line}");
    // Once: the watch is over, and a second reading says nothing more.
    assert!(WATCHES.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A test's own file for the watches, taken away at the end: no test writes
/// under `/run`.
struct Kept {
    dir: PathBuf,
}

impl Kept {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/vwatch-file-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        *FILE_AT.lock().unwrap() = Some(dir.join("watches.json"));
        Self { dir }
    }

    fn file(&self) -> PathBuf {
        self.dir.join("watches.json")
    }

    /// A new worker: this process's watches are gone, the file's are taken up.
    fn new_worker(&self) -> usize {
        *WATCHES.lock().unwrap() = Watches::default();
        restore(lamp_ipc::monotonic_us())
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        *FILE_AT.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn the_file_keeps_facts_only_at_0600() {
    use std::os::unix::fs::PermissionsExt as _;
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("facts");
    WATCHES.lock().unwrap().stop_all();
    assert!(
        run(
            WATCH,
            &json!({"what": "co2_above", "ppm": 1000, "for_hours": 3})
        )
        .1
    );
    assert!(run(WATCH, &json!({"what": "arrives"})).1);
    let mode = std::fs::metadata(kept.file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    let text = std::fs::read_to_string(kept.file()).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    let list = value["watches"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    for entry in list {
        let mut keys: Vec<&str> = entry
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["kind", "n", "until_us"], "{entry}");
    }
    assert!(!text.contains("tell you"), "no words said: {text}");
    assert!(!kept.dir.join("watches.json.new").exists());
}

#[test]
fn a_stopped_watch_stays_stopped_in_the_next_worker() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("stopped");
    WATCHES.lock().unwrap().stop_all();
    assert!(run(WATCH, &json!({"what": "co2_above"})).1);
    assert!(run(WATCH, &json!({"what": "leaves"})).1);
    assert_eq!(kept.new_worker(), 2, "a cycle ends: both are still there");
    assert_eq!(run(STOP, &json!({})).0["say"], STOPPED);
    assert_eq!(kept.new_worker(), 0, "stopped is stopped");
    assert_eq!(run(STOP, &json!({})).0["say"], NOTHING);
}

#[test]
fn a_file_that_is_wrong_is_no_watches_and_never_a_crash() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("wrong");
    for bad in [
        "".to_owned(),
        "not json".to_owned(),
        "[1,2]".to_owned(),
        r#"{"watches": 5}"#.to_owned(),
        r#"{"watches": [{"kind": "co2_above", "n": 99999999999, "until_us": 1}]}"#.to_owned(),
    ] {
        std::fs::write(kept.file(), &bad).unwrap();
        assert_eq!(kept.new_worker(), 0, "{bad}");
    }
    // A bad entry among good ones is left out, the good ones kept.
    let now = lamp_ipc::monotonic_us();
    let text = json!({"watches": [
        {"kind": "co2_above", "n": 1200, "until_us": now + 60 * SECOND},
        {"kind": "nonsense", "n": 0, "until_us": now + 60 * SECOND},
        {"kind": "leaves", "n": 0, "until_us": 1},
        {"kind": "arrives", "n": 0},
    ]});
    std::fs::write(kept.file(), text.to_string()).unwrap();
    assert_eq!(kept.new_worker(), 1);
    // And no more than five, none longer than a day.
    let many: Vec<Value> = (0..9)
        .map(|_| json!({"kind": "arrives", "n": 0, "until_us": now + 100 * LONGEST_US}))
        .collect();
    std::fs::write(kept.file(), json!({"watches": many}).to_string()).unwrap();
    assert_eq!(kept.new_worker(), MOST);
    assert!(
        WATCHES
            .lock()
            .unwrap()
            .list
            .iter()
            .all(|watch| watch.until_us <= lamp_ipc::monotonic_us() + LONGEST_US)
    );
    WATCHES.lock().unwrap().stop_all();
}

#[test]
fn forgetting_everything_ends_the_watches_and_removes_the_file() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("forgot");
    WATCHES.lock().unwrap().stop_all();
    assert!(run(WATCH, &json!({"what": "sitting_over", "minutes": 30})).1);
    assert!(kept.file().exists());
    // The voice's own `forget_everything`, once done.
    forget_all();
    assert!(WATCHES.lock().unwrap().is_empty());
    assert!(!kept.file().exists());
    assert_eq!(kept.new_worker(), 0);
    // Forgetting again, with nothing kept, is no error.
    forget_all();
}

/// A watch kept across a cycle must not fire on who was in the room before
/// the new worker started (Sixth's P2 on #7782): the first look at the faces
/// after a start is the baseline, told to no one.
#[test]
fn a_restored_arrives_or_turns_watch_does_not_fire_on_someone_already_there() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("baseline");
    WATCHES.lock().unwrap().stop_all();
    assert!(run(WATCH, &json!({"what": "arrives"})).1);
    assert!(run(WATCH, &json!({"what": "turns_to_lamp"})).1);
    // The cycle ends; the new worker restores both, knowing nothing of the room.
    assert_eq!(kept.new_worker(), 2);
    let people = |count, facing| Reading::People { count, facing };
    let now = lamp_ipc::monotonic_us();
    let mut watches = WATCHES.lock().unwrap();
    // Someone was already there, looking at the lamp: nothing is told.
    assert!(watches.read(&people(1, true), now, true).is_empty());
    assert!(
        watches
            .read(&people(1, true), now + SECOND, true)
            .is_empty()
    );
    assert_eq!(watches.len(), 2);
    // They turn away and back: the turn is news; later someone comes after a
    // gap of nobody: that is an arrival.
    assert!(
        watches
            .read(&people(1, false), now + 2 * SECOND, true)
            .is_empty()
    );
    assert_eq!(
        watches.read(&people(1, true), now + 3 * SECOND, true).len(),
        1
    );
    assert!(
        watches
            .read(&people(0, false), now + 4 * SECOND, true)
            .is_empty()
    );
    assert!(
        watches
            .read(&people(0, false), now + 10 * SECOND, true)
            .is_empty()
    );
    assert_eq!(
        watches
            .read(&people(1, false), now + 20 * SECOND, true)
            .len(),
        1
    );
    assert!(watches.is_empty());
    drop(watches);
    drop(kept);
}

/// A cycle ends (a mute, a hand's sleep), a new worker starts, and the watch
/// taken before is still there and is told once, with the file after it
/// holding none.
#[test]
fn a_watch_survives_a_cycle_and_is_told_once_by_the_next_worker() {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixListener;
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kept = Kept::new("cycle");
    WATCHES.lock().unwrap().stop_all();
    // The first worker takes the watch.
    assert!(run(WATCH, &json!({"what": "co2_above", "ppm": 1000})).1);
    // Its cycle ends; the next worker starts, takes up the file, then the
    // watcher runs.
    assert_eq!(kept.new_worker(), 1);
    let dir = kept.dir.clone();
    let bus = Bus::new(dir.join("bus")).expect("a bus");
    let mut air = bus
        .publisher_declared(AirState::TOPIC, &AirState::declaration(), "air")
        .expect("a publisher");
    let socket = dir.join("say.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (heard, got) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            let mut line = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            if reader.read_line(&mut line).is_ok() {
                let _ = heard.send(line.clone());
                let mut stream = stream;
                let _ = writeln!(stream, "{{\"outcome\":\"spoken\"}}");
            }
        }
    });
    let _watcher = start_on(&bus, socket).expect("the watcher starts");
    let reading = |ppm: u16| AirState {
        pm1_0: None,
        pm2_5: None,
        pm4_0: None,
        pm10: None,
        humidity: None,
        temperature: None,
        co2_ppm: Some(ppm),
        feel: lamp_msgs::AirFeel::Unknown,
    };
    air.publish_json(lamp_ipc::monotonic_us(), &reading(1_250))
        .unwrap();
    let line = got.recv_timeout(Duration::from_secs(2)).expect("the note");
    assert!(line.contains("1250"), "{line}");
    // Once: a second reading says nothing more.
    air.publish_json(lamp_ipc::monotonic_us(), &reading(1_300))
        .unwrap();
    assert!(got.recv_timeout(Duration::from_millis(700)).is_err());
    // And the file no longer holds it: a worker after this one has none.
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while kept.new_worker() != 0 && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(kept.new_worker(), 0);
}
