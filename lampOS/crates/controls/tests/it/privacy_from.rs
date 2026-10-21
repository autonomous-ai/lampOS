//! `serve --privacy-from FILE` (#2602): the privacy slide's reading from a test
//! input on a lamp, so the rows that need the switch moved can run without a
//! hand on it. Test-only: no product unit passes the flag.
use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    privacy::{FileReader, POLL_US, Reader, Source},
    runtime::{MissingPins, Service},
};
use lamp_msgs::{ControlFault, ControlsState, PRIVACY_SWITCH_TTL_US};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "privacy-test";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Short: a subscriber's socket path must fit in 103 bytes.
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/cpf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn say(&self, what: &str) -> PathBuf {
        let file = self.0.join("privacy");
        fs::write(&file, what).unwrap();
        file
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

/// Each word the file can say, read at the slide's own poll: open and closed
/// as the slide, `gap` as no reading at all, anything else unreadable.
#[test]
fn the_file_reads_as_the_slide_and_gap_takes_no_reading() {
    let root = Root::new();
    let file = root.say("closed\n");
    let mut reader = FileReader::new(&file, CLOCK, "test-input");
    let first = reader.read(0).unwrap();
    assert_eq!((first.muted, first.fault), (Some(true), None));
    assert_eq!(first.stamp.sequence, 1);
    // Not again before the poll.
    assert!(reader.read(POLL_US - 1).is_none());
    root.say("open");
    let open = reader.read(POLL_US).unwrap();
    assert_eq!((open.muted, open.fault), (Some(false), None));
    assert_eq!(open.stamp.sequence, 2);
    root.say("gap");
    assert!(reader.read(2 * POLL_US).is_none());
    assert!(reader.read(3 * POLL_US).is_none());
    for word in ["fault", "", "OPEN", "half"] {
        root.say(word);
        let read = reader.read(10 * POLL_US).unwrap();
        assert_eq!(
            (read.muted, read.fault),
            (None, Some(ControlFault::Missing)),
            "{word:?}"
        );
        reader = FileReader::new(&file, CLOCK, "test-input");
    }
    fs::remove_file(&file).unwrap();
    let missing = reader.read(0).unwrap();
    assert_eq!(
        (missing.muted, missing.fault),
        (None, Some(ControlFault::Missing))
    );
}

/// On the bus, as the voice reads it: `controls/state` follows the file, and a
/// gap lets the reading go stale past its life, as a stalled loop did on
/// lamp-75c9 (21 Oct), until the file says open again.
#[test]
fn controls_state_follows_the_file_and_a_gap_goes_stale() {
    let root = Root::new();
    let bus = Bus::new(root.0.join("bus")).unwrap();
    let file = root.say("closed");
    let mut service = Service::new(&bus, Config::default(), MissingPins, CLOCK).unwrap();
    service.attach_privacy(Source::new(
        Box::new(FileReader::new(&file, CLOCK, "test-input")),
        CLOCK,
    ));
    let latest = || -> ControlsState {
        bus.latest(ControlsState::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap()
    };
    service.step(|| 0);
    assert_eq!(latest().privacy.unwrap().muted, Some(true));
    root.say("open");
    service.step(|| 6_000);
    let state = latest();
    state.check().unwrap();
    assert_eq!(state.privacy.unwrap().muted, Some(false));
    // A gap: no reading, so the last one ages out.
    root.say("gap");
    let mut at = 6_000;
    while at < 6_000 + PRIVACY_SWITCH_TTL_US + 30_000 {
        at += 1_000;
        service.step(|| at);
    }
    assert_eq!(latest().privacy.unwrap().fault, Some(ControlFault::Stale));
    root.say("open");
    at += POLL_US;
    service.step(|| at);
    at += 20_000;
    service.step(|| at);
    let back = latest().privacy.unwrap();
    assert_eq!((back.muted, back.fault), (Some(false), None));
}

/// The flag stands in for a slide only, and says so in the journal.
#[test]
fn serve_takes_the_flag_only_with_the_route_and_says_it_is_a_test_input() {
    let root = Root::new();
    let file = root.say("open");
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
                "--privacy-from",
            ])
            .arg(&file);
        command
    };
    // Without the route: refused.
    let off = serve().output().unwrap();
    assert!(!off.status.success());
    assert!(
        String::from_utf8_lossy(&off.stderr)
            .contains("--privacy-from needs LAMP_PRIVACY_CONTROLS=on"),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    // With it and a privacy section: served from the file, no chip opened.
    let on = serve()
        .env("LAMP_PRIVACY_CONTROLS", "on")
        .args(["--touch", "--config"])
        .arg(fixture("privacy-controls.json"))
        .arg("--pins")
        .arg(fixture("privacy-pins.json"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&on.stderr);
    assert!(on.status.success(), "{said}");
    assert!(
        said.contains("lamp-controls: privacy from a test input:"),
        "{said}"
    );
}

/// No unit lampOS deploys passes the test flag.
#[test]
fn no_deployed_unit_passes_privacy_from() {
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
    let controls: Vec<_> = units
        .iter()
        .filter(|unit| {
            unit.file_name()
                .is_some_and(|name| name == "lamp-controls.service")
        })
        .collect();
    assert!(
        !controls.is_empty(),
        "no lamp-controls unit found under deploy/"
    );
    for unit in &units {
        let text = fs::read_to_string(unit).unwrap();
        assert!(
            !text.contains("--privacy-from"),
            "{} passes --privacy-from",
            unit.display()
        );
    }
}
