//! `serve --touch --touch-from FILE` (#2602): the head pads read from a test
//! input on a lamp, so a run that needs the head touched needs no hand on it.
//! Only the lines are swapped; the gesture behind them is the real one.
//! Test-only: no product unit passes the flag.
use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    pins::{FileLines, LinePins, Lines, Pin, Pull},
    strip_worker::NoStrip,
    touch_pins::TouchPinsConfig,
    touch_runtime::Service,
};
use lamp_msgs::{TouchEvent, TouchGesture, TouchIntent, TouchSource, TouchState};
use std::{
    cell::Cell,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const CLOCK: &str = "touch-from-test";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Short: a subscriber's socket path must fit in 103 bytes.
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/ctf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("head")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// lamp-52e6's two head pads (#1362), driven high by a touch.
fn pads() -> Vec<Pin> {
    let pad = |name: &str, line| Pin {
        name: name.into(),
        line,
        active_low: false,
        pull: Pull::Down,
    };
    vec![pad("head-3", 97), pad("head-4", 98)]
}

fn wiring() -> TouchPinsConfig {
    TouchPinsConfig {
        privacy: None,
        chip: "/dev/gpiochip0".into(),
        buttons: vec![],
        head_pads: pads(),
        reset: None,
        every_ms: 5,
        base: None,
        head_qualify_ms: None,
        head_tap_min_ms: None,
        head_gap_ms: None,
        head_rest: None,
        legacy_head: false,
    }
}

fn config() -> Config {
    Config {
        enabled: false,
        buttons: vec![],
        head_pads: vec!["head-3".into(), "head-4".into()],
        base_selected_mask: None,
    }
}

/// The GPIO lines, as a pad drives them: line 97 high while touched.
#[derive(Clone, Default)]
struct Gpio(Rc<Cell<bool>>);
impl Lines for Gpio {
    fn high(&mut self, line: u32) -> io::Result<bool> {
        Ok(line == 97 && self.0.get())
    }
}

/// Touch: none for 1 s, past the head settling, held 3.5 s (sleep), let go
/// 1 s, then a 0.2 s tap (wake), every 5 ms as the pads are polled. Calls `touch` at each change.
fn run<L: Lines>(lines: L, mut touch: impl FnMut(bool)) -> Vec<TouchEvent> {
    let root = Root::new();
    let bus = Bus::new(root.0.join("bus"))
        .unwrap()
        .rescan_every(Duration::ZERO);
    let (pins, controls) = wiring().gpio_config().unwrap();
    let reader = LinePins::new(lines, pins, &controls, CLOCK, "reader".into()).unwrap();
    let mut service =
        Service::new(&bus, config(), wiring(), reader, None::<NoStrip>, CLOCK).unwrap();
    let start = 1_000_000;
    let mut events = Vec::new();
    let mut was = None;
    for step in 0..1_300u64 {
        let at = start + step * 5_000;
        let since = at - start;
        let touched =
            (1_000_000..4_500_000).contains(&since) || (5_500_000..5_700_000).contains(&since);
        if was != Some(touched) {
            touch(touched);
            was = Some(touched);
        }
        let output = service.step(|| at);
        output.state.check().unwrap();
        for mut event in output.events {
            event.check().unwrap();
            // Each run's own Controls session.
            event.session_id.clear();
            events.push(event);
        }
    }
    events
}

/// A hold from the file is the sleep the lines give, and a tap the wake:
/// the same events, at the same times, through the same gesture.
#[test]
fn a_hold_and_a_tap_from_the_file_are_the_events_the_lines_give() {
    let head = Gpio::default();
    let wired = run(head.clone(), |touched| head.0.set(touched));
    let root = Root::new();
    let file = root.file();
    let from_file = run(FileLines::new(&file, pads(), None), |touched| {
        fs::write(&file, if touched { "head-3\n" } else { "none\n" }).unwrap()
    });
    assert_eq!(from_file, wired);
    let head_events: Vec<_> = wired
        .iter()
        .filter(|event| event.source == TouchSource::Head)
        .collect();
    let hold = head_events
        .iter()
        .find(|event| event.gesture == TouchGesture::Hold)
        .expect("a hold");
    assert_eq!(hold.when_quiet, TouchIntent::Sleep);
    assert_eq!(hold.active_names, ["head-3"]);
    let tap = head_events
        .iter()
        .find(|event| event.gesture == TouchGesture::Tap)
        .expect("a tap");
    assert_eq!(tap.when_asleep, Some(TouchIntent::Wake));
}

/// The words FILE takes: pad names, `none` or nothing; a missing file or
/// another word fails the read, as a lost chip does.
#[test]
fn the_file_names_the_touched_pads_or_none() {
    let root = Root::new();
    let file = root.file();
    let mut lines = FileLines::new(&file, pads(), None);
    assert!(lines.high(97).is_err(), "a missing file");
    for (said, high) in [
        ("none", [false, false]),
        ("", [false, false]),
        ("head-3", [true, false]),
        ("head-3 head-4\n", [true, true]),
        ("head-4", [false, true]),
    ] {
        fs::write(&file, said).unwrap();
        assert_eq!(
            [lines.high(97).unwrap(), lines.high(98).unwrap()],
            high,
            "{said:?}"
        );
    }
    fs::write(&file, "both").unwrap();
    assert!(lines.high(97).is_err());
    // A line not a head pad's, with no other lines: not read.
    assert!(lines.high(99).is_err());
}

/// The flag needs --touch and head pads, says so in the journal, and the
/// touch state names the source as a test input.
#[test]
fn serve_takes_the_flag_with_touch_and_names_the_test_input() {
    let root = Root::new();
    let file = root.file();
    fs::write(&file, "none").unwrap();
    let pins = root.0.join("pins.json");
    let controls = root.0.join("controls.json");
    fs::write(&pins, serde_json::to_string(&wiring()).unwrap()).unwrap();
    fs::write(&controls, serde_json::to_string(&config()).unwrap()).unwrap();
    let serve = || {
        let mut command = Command::new(PROGRAM);
        command
            .env_remove("LAMP_PRIVACY_CONTROLS")
            .env("LAMP_BUS_ROOT", root.0.join("bus"))
            .args([
                "serve",
                "--clock-id",
                CLOCK,
                "--seconds",
                "1",
                "--touch-from",
            ])
            .arg(&file);
        command
    };
    let off = serve().output().unwrap();
    assert!(!off.status.success());
    assert!(
        String::from_utf8_lossy(&off.stderr).contains("--touch-from needs --touch"),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    let on = serve()
        .args(["--touch", "--config"])
        .arg(&controls)
        .arg("--pins")
        .arg(&pins)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&on.stderr);
    assert!(on.status.success(), "{said}");
    assert!(
        said.contains("lamp-controls: head pads from a test input:"),
        "{said}"
    );
    let state: TouchState = Bus::new(root.0.join("bus"))
        .unwrap()
        .latest(TouchState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    let head = state
        .sources
        .iter()
        .find(|source| source.source == TouchSource::Head)
        .unwrap();
    assert!(
        head.reader_session_id
            .as_deref()
            .is_some_and(|session| session.starts_with("touch-test-input-")),
        "{:?}",
        head.reader_session_id
    );
}

/// No unit lampOS deploys passes the test flag.
#[test]
fn no_deployed_unit_passes_touch_from() {
    let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
    let mut units = Vec::new();
    let mut folders = vec![deploy];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext == "service") {
                units.push(path);
            }
        }
    }
    assert!(
        units.iter().any(|unit| unit
            .file_name()
            .is_some_and(|name| name == "lamp-controls.service")),
        "no lamp-controls unit found under deploy/"
    );
    for unit in &units {
        let text = fs::read_to_string(unit).unwrap();
        assert!(
            !text.contains("--touch-from"),
            "{} passes --touch-from",
            unit.display()
        );
        // discover-head's test input (#2818), likewise.
        assert!(
            !text.contains("--lines-from"),
            "{} passes --lines-from",
            unit.display()
        );
    }
}
