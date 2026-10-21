//! `LAMP_PRIVACY_CONTROLS=no-slide`: the privacy route on a lamp with no
//! slide. Controls reads no line and says so in each `controls/state`, at the
//! route's cadence, so the voice can grant from its own mute alone.
use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    privacy::{self, LineReader, Source},
    runtime::{MissingPins, POLL_US, Service},
    strip_worker::Worker,
    touch_pins::{NoGpio, TouchPinsConfig},
    touch_runtime,
};
use lamp_msgs::ControlsState;
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const CLOCK: &str = "privacy-test";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Short: a subscriber's socket path must fit in 103 bytes, which the Mac's
/// temporary folder leaves no room for.
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/cns-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn bus(&self) -> Bus {
        Bus::new(self.0.join("bus")).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

struct Low;
impl lamp_controls::pins::Lines for Low {
    fn high(&mut self, _: u32) -> io::Result<bool> {
        Ok(false)
    }
}

fn slide() -> Source {
    Source::new(
        Box::new(LineReader::new(Low, CLOCK, "privacy-reader")),
        CLOCK,
    )
}

/// Every `controls/state` received in the second half of two seconds of
/// steps, each checked to carry the marker and no slide evidence.
fn second_of_states(mut step: impl FnMut(u64), bus: &Bus, every_us: u64) -> usize {
    let mut states = bus
        .subscribe(ControlsState::TOPIC, "test_no_slide")
        .unwrap();
    let mut count = 0;
    for at in (0..2_000_000u64).step_by(every_us as usize) {
        step(at);
        if at == 1_000_000 {
            count = 0;
        }
        while let Some(message) = states.recv(Duration::ZERO).unwrap() {
            let state: ControlsState = message.parse().unwrap();
            state.check().unwrap();
            assert!(state.no_privacy_slide, "{state:?}");
            assert!(state.privacy.is_none(), "{state:?}");
            count += 1;
        }
    }
    count
}

#[test]
fn only_exact_no_slide_selects_the_route_with_no_slide() {
    for value in [
        None,
        Some("on"),
        Some("off"),
        Some("NO-SLIDE"),
        Some(""),
        Some("no_slide"),
    ] {
        assert!(!privacy::no_slide(value.map(OsStr::new)), "{value:?}");
    }
    assert!(privacy::no_slide(Some(OsStr::new("no-slide"))));
    assert!(!privacy::selected(Some(OsStr::new("no-slide"))));
}

/// The voice reads `controls/state` against a 50 ms freshness: with no slide
/// it is said every 20 ms, as with a slide reader, not every 100 ms.
#[test]
fn with_no_slide_a_still_second_says_the_marker_fifty_times() {
    let root = Root::new();
    let bus = root.bus();
    let mut service = Service::new(&bus, Config::default(), MissingPins, CLOCK).unwrap();
    assert_eq!(service.poll_us(), POLL_US);
    service.mark_no_slide().unwrap();
    assert_eq!(service.poll_us(), privacy::STEP_US);
    let count = second_of_states(
        |at| {
            let state = service.step(|| at).state;
            state.check().unwrap();
            assert!(state.no_privacy_slide && state.privacy.is_none());
        },
        &bus,
        privacy::STEP_US,
    );
    assert!((45..=55).contains(&count), "{count} controls states");
}

/// The touch path lamps run (`serve --touch`): with no button in the pins, a
/// state owner is made for the marker, as for a slide reader.
#[test]
fn with_no_slide_the_touch_path_says_the_marker_fifty_times() {
    let root = Root::new();
    let bus = root.bus();
    let wiring: TouchPinsConfig = serde_json::from_str(include_str!(
        "../../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    assert!(wiring.buttons.is_empty() && wiring.privacy.is_none());
    let config = Config {
        base_selected_mask: Some(
            wiring
                .base
                .as_ref()
                .unwrap()
                .pads
                .iter()
                .fold(0, |mask, &pad| mask | (1 << pad)),
        ),
        ..Config::default()
    };
    let mut service =
        touch_runtime::Service::new(&bus, config, wiring, NoGpio, None::<Worker>, CLOCK).unwrap();
    assert_eq!(service.poll_us(), POLL_US);
    service.mark_no_slide(&bus, CLOCK).unwrap();
    assert_eq!(service.poll_us(), privacy::STEP_US);
    service.strip_unavailable(0, "no strip");
    let count = second_of_states(
        |at| {
            service.step(|| at);
        },
        &bus,
        2_000,
    );
    assert!((45..=55).contains(&count), "{count} controls states");
}

#[test]
fn no_slide_is_refused_beside_a_slide_reader() {
    let root = Root::new();
    let bus = root.bus();
    let mut service = Service::new(&bus, Config::default(), MissingPins, CLOCK).unwrap();
    service.attach_privacy(slide());
    assert!(service.mark_no_slide().is_err());
    let state = service.step(|| 0).state;
    assert!(!state.no_privacy_slide && state.privacy.is_some());
    drop(service);
    let wiring: TouchPinsConfig = serde_json::from_str(include_str!(
        "../../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    let config = Config {
        base_selected_mask: wiring
            .base
            .as_ref()
            .map(|base| base.pads.iter().fold(0, |mask, &pad| mask | (1 << pad))),
        ..Config::default()
    };
    let mut touch =
        touch_runtime::Service::new(&bus, config, wiring, NoGpio, None::<Worker>, CLOCK).unwrap();
    touch.attach_privacy(&bus, slide()).unwrap();
    assert!(touch.mark_no_slide(&bus, CLOCK).is_err());
}

fn command(root: &Root, value: &str) -> Command {
    let mut command = Command::new(PROGRAM);
    command
        .env("LAMP_PRIVACY_CONTROLS", value)
        .env("LAMP_BUS_ROOT", root.0.join("must-not-exist"));
    command
}

/// A privacy section says a slide is wired: that lamp is `on`. With
/// `no-slide` Controls refuses to start rather than leave its slide unread.
#[test]
fn no_slide_refuses_a_pins_file_with_a_privacy_section() {
    let root = Root::new();
    let mut with_hands: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("strip-711-pins-example.json")).unwrap()).unwrap();
    with_hands["privacy"] = serde_json::json!(
        {"chip": "/dev/gpiochip1", "line": 9, "active_low": true, "pull": "up"}
    );
    let hands = root.0.join("hands.json");
    fs::write(&hands, serde_json::to_vec(&with_hands).unwrap()).unwrap();
    for pins in [fixture("privacy-pins.json"), hands] {
        let output = command(&root, "no-slide")
            .args(["replay", "--clock-id", CLOCK, "--config"])
            .arg(fixture("privacy-controls.json"))
            .arg("--input")
            .arg(fixture("privacy-touch-replay.jsonl"))
            .args(["--touch", "--pins"])
            .arg(&pins)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{}: {stderr}", pins.display());
        assert!(output.stdout.is_empty());
        assert!(
            stderr.contains(
                "LAMP_PRIVACY_CONTROLS=no-slide, but the pins file has a privacy section"
            ),
            "{stderr}"
        );
    }
    assert!(!root.0.join("must-not-exist").exists());
}

/// The real program: its start line says the route is on with no slide, and
/// the state it leaves on the bus carries the marker and no slide evidence.
/// With `on` and no privacy section it is as before: said, and no marker.
#[test]
fn serve_with_no_slide_says_so_and_marks_its_state() {
    for (value, line, marked) in [
        (
            "no-slide",
            "lamp-controls: privacy route on, no slide: the voice grants from its mute alone",
            true,
        ),
        (
            "on",
            "lamp-controls: LAMP_PRIVACY_CONTROLS=on but the pins file has no privacy section: no privacy source",
            false,
        ),
    ] {
        let root = Root::new();
        let output = command(&root, value)
            .args(["serve", "--clock-id", CLOCK, "--seconds", "1", "--bus-root"])
            .arg(root.0.join("bus"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{value}: {stderr}");
        assert!(stderr.contains(line), "{value}: {stderr}");
        let state: ControlsState = root
            .bus()
            .latest(ControlsState::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        state.check().unwrap();
        assert_eq!(state.no_privacy_slide, marked, "{value}");
        assert!(state.privacy.is_none(), "{value}");
    }
}
