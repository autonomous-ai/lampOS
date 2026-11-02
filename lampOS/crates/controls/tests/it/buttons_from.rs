//! `serve --touch --buttons-from FILE` (#3697): the primary button and reset
//! read from a test input on a lamp, so the long press that turns the lamp
//! off can be tried with no hand on it. Only the lines are swapped; the hold,
//! its warning and the request behind them are the real ones. Test-only: no
//! product unit passes the flag.
use lamp_controls::{
    engine::Config,
    input::PinReader,
    pins::{FileLines, LinePins, Lines, Pin, Pull},
    touch::{TouchEngine, shutdown_request},
    touch_pins::TouchPinsConfig,
};
use lamp_msgs::{ShutdownSource, TouchEvent, TouchIntent};
use std::{
    cell::Cell,
    fs, io,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/cbf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The board's primary button (line 99) and reset (line 100), both pulled
/// up and pressed low.
fn buttons() -> Vec<Pin> {
    let pin = |name: &str, line| Pin {
        name: name.into(),
        line,
        active_low: true,
        pull: Pull::Up,
    };
    vec![pin("primary", 99), pin("reset", 100)]
}

fn wiring(head_pads: Vec<Pin>) -> TouchPinsConfig {
    let mut pins = buttons();
    let reset = pins.pop();
    TouchPinsConfig {
        privacy: None,
        chip: "/dev/gpiochip0".into(),
        buttons: pins,
        head_pads,
        reset,
        every_ms: 5,
        base: None,
        head_qualify_ms: None,
        head_tap_min_ms: None,
        head_gap_ms: None,
        strip_qualify_ms: None,
        strip_release_ms: None,
        strip_stroke_ms: None,
        strip_sleep_ms: None,
        head_rest: None,
        legacy_head: false,
    }
}

fn config() -> Config {
    Config {
        enabled: true,
        buttons: vec!["primary".into()],
        head_pads: vec![],
        base_selected_mask: None,
    }
}

/// The GPIO lines, as the buttons drive them: line 99 low while pressed.
#[derive(Clone, Default)]
struct Gpio(Rc<Cell<bool>>);
impl Lines for Gpio {
    fn high(&mut self, line: u32) -> io::Result<bool> {
        Ok(!(line == 99 && self.0.get()))
    }
}

/// The primary button: up 1 s, held 6 s, then up 1 s, every 5 ms as the
/// buttons are polled, on an engine with the shutdown switch on. Calls
/// `press` at each change.
fn run<L: Lines>(lines: L, mut press: impl FnMut(bool)) -> Vec<TouchEvent> {
    let wiring = wiring(vec![]);
    let touch = wiring.check(&config()).unwrap();
    let (pins, controls) = wiring.gpio_config().unwrap();
    let mut reader = LinePins::new(lines, pins, &controls, "boot", "reader".into()).unwrap();
    let mut engine = TouchEngine::new(touch, "boot", "controls").unwrap();
    engine.set_shutdown(true);
    let start = 1_000_000;
    let mut events = Vec::new();
    let mut was = None;
    for step in 0..1_600u64 {
        let at = start + step * 5_000;
        let pressed = (1_000_000..7_000_000).contains(&(at - start));
        if was != Some(pressed) {
            press(pressed);
            was = Some(pressed);
        }
        if let Some(observation) = reader.read(at).unwrap() {
            engine
                .ingest_gpio(at, wiring.gpio_sample(observation).unwrap())
                .unwrap();
        }
        let output = engine.tick(at);
        output.state.check().unwrap();
        for event in output.events {
            event.check().unwrap();
            events.push(event);
        }
    }
    events
}

/// The primary button held 6 s in the file, then `none`, asks to turn the
/// lamp off exactly as the held line does: the same events, at the same
/// times, and one request from the button.
#[test]
fn a_six_second_hold_from_the_file_asks_the_shutdown_the_lines_do() {
    let button = Gpio::default();
    let wired = run(button.clone(), |pressed| button.0.set(pressed));
    let root = Root::new();
    let file = root.0.join("buttons");
    let from_file = run(FileLines::new(&file, buttons(), None), |pressed| {
        fs::write(&file, if pressed { "primary\n" } else { "none\n" }).unwrap()
    });
    assert_eq!(from_file, wired);
    assert_eq!(
        wired
            .iter()
            .map(|event| event.when_quiet)
            .collect::<Vec<_>>(),
        [TouchIntent::Shutdown]
    );
    let requests: Vec<_> = wired.iter().filter_map(shutdown_request).collect();
    assert_eq!(requests.len(), 1, "exactly one request");
    assert_eq!(requests[0].source, ShutdownSource::Button);
}

/// The flag needs --touch, and a button or reset in --pins; it says so.
#[test]
fn serve_takes_the_flag_only_with_touch_and_a_button() {
    let root = Root::new();
    let file = root.0.join("buttons");
    fs::write(&file, "none").unwrap();
    let pins = root.0.join("pins.json");
    let controls = root.0.join("controls.json");
    let pad = Pin {
        name: "head-3".into(),
        line: 97,
        active_low: false,
        pull: Pull::Down,
    };
    let mut head_only = wiring(vec![pad]);
    head_only.buttons.clear();
    head_only.reset = None;
    fs::write(&pins, serde_json::to_string(&head_only).unwrap()).unwrap();
    let head_config = Config {
        buttons: vec![],
        head_pads: vec!["head-3".into()],
        ..config()
    };
    fs::write(&controls, serde_json::to_string(&head_config).unwrap()).unwrap();
    let serve = |touch: bool| {
        let mut command = Command::new(PROGRAM);
        command
            .env_remove("LAMP_PRIVACY_CONTROLS")
            .env("LAMP_BUS_ROOT", root.0.join("bus"))
            .args(["serve", "--clock-id", "buttons-from-test", "--seconds", "1"])
            .arg("--buttons-from")
            .arg(&file);
        if touch {
            command
                .args(["--touch", "--config"])
                .arg(&controls)
                .arg("--pins")
                .arg(&pins);
        }
        command.output().unwrap()
    };
    for output in [serve(false), serve(true)] {
        assert!(!output.status.success());
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(
            said.contains("--buttons-from needs --touch and a button or reset in --pins"),
            "{said}"
        );
    }
}
