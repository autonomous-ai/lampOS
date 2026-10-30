//! Names and the time zone changed after setup (#1959): root's helper,
//! `lamp-setup change`, against files in a folder of the test's own.
use lamp_msgs::Voice;
use lamp_setup::change::{self, Change, Ears, answer, parse, wait_for_quiet};
use lamp_setup::serve::Config;
use lamp_update::quiet::Heard;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lamp-setup-change-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("zoneinfo/Asia")).unwrap();
        fs::create_dir_all(path.join("zoneinfo/Europe")).unwrap();
        fs::write(path.join("zoneinfo/Asia/Ho_Chi_Minh"), b"TZif").unwrap();
        fs::write(path.join("zoneinfo/Europe/Paris"), b"TZif").unwrap();
        // In the lamp's zone data, not in the parts' bundled one.
        fs::write(path.join("zoneinfo/Asia/Nowhere"), b"TZif").unwrap();
        fs::create_dir_all(path.join("etc")).unwrap();
        Self(path)
    }

    fn config(&self) -> Config {
        serde_json::from_value(json!({
            "listen": "127.0.0.1:5000",
            "marker": self.0.join("etc/setup-done"),
            "credential": self.0.join("etc/credential"),
            "policy_key": self.0.join("etc/jev.key"),
            "name": self.0.join("etc/person/name"),
            "country": "VN",
            "zoneinfo": self.0.join("zoneinfo"),
            "localtime": self.0.join("etc/localtime"),
            "timezone_file": self.0.join("etc/timezone"),
            "zone_env": self.0.join("etc/zone.env"),
            "backend": self.0.join("etc/backend.json"),
            "reports": self.0.join("etc/reports.json"),
            "display_name": self.0.join("etc/display-name"),
            "control_dropin": self.0.join("etc/systemd/system/lamp-control.service.d/developer.conf"),
        }))
        .unwrap()
    }

    /// Every file under the folder but the zone data, with its bytes.
    fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut found = Vec::new();
        let mut folders = vec![self.0.join("etc")];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    folders.push(path);
                } else if meta.file_type().is_symlink() {
                    let target = fs::read_link(&path).unwrap();
                    found.push((path, target.into_os_string().into_encoded_bytes()));
                } else {
                    found.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        found.sort();
        found
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn the_persons_name_is_written_as_setup_writes_it_and_an_empty_one_removes_it() {
    let root = Root::new();
    let config = root.config();
    let (reply, done) = answer(r#"{"person_name": "  Lan  "}"#, &config, true);
    assert_eq!(reply, json!({"done": {"name": "Lan"}}));
    assert!(!done.unwrap().restart_schedule);
    assert_eq!(fs::read_to_string(&config.name).unwrap(), "Lan\n");
    assert_eq!(mode(&config.name), 0o600);
    let (reply, _) = answer(r#"{"person_name": "Minh Anh"}"#, &config, true);
    assert_eq!(reply["done"]["name"], "Minh Anh");
    assert_eq!(fs::read_to_string(&config.name).unwrap(), "Minh Anh\n");
    let (reply, done) = answer(r#"{"person_name": ""}"#, &config, true);
    assert_eq!(reply, json!({"done": {"name": ""}}));
    assert_eq!(done.unwrap().said, "the person's name was removed");
    assert!(!config.name.exists());
    // Removing it again is no error.
    let (reply, _) = answer(r#"{"person_name": " "}"#, &config, true);
    assert_eq!(reply, json!({"done": {"name": ""}}));
}

/// #4333: the owner's place, rounded again here whatever was sent, is kept
/// beside the person's name, root's 0600, and `null` clears it; a clear
/// with none kept is no error. The journal says only that it changed, and
/// a refusal holds neither the name nor where.
#[test]
fn the_owners_place_is_kept_rounded_beside_the_name_and_null_clears_it() {
    let root = Root::new();
    let config = root.config();
    let place = lamp_msgs::owner_place::beside_name(&config.name);
    assert_eq!(place, root.0.join("etc/person/place"));
    let line = json!({"owner_place": {
        "name": "Ba Dinh, Hanoi", "lat": 21.035_123, "lon": 105.834_987,
        "set_at": "2026-10-29T08:30:00Z", "street": "12 Nowhere Street"
    }})
    .to_string();
    let (reply, done) = answer(&line, &config, true);
    let kept = json!({"name": "Ba Dinh, Hanoi", "lat": 21.04, "lon": 105.83, "set_at": "2026-10-29T08:30:00Z"});
    assert_eq!(reply, json!({ "done": kept }));
    assert_eq!(done.unwrap().said, "the owner's place was set");
    let written = fs::read_to_string(&place).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap(), kept);
    assert!(!written.contains("21.035") && !written.contains("Street"));
    assert_eq!(mode(&place), 0o600);
    // The voice reads it as kept.
    assert_eq!(
        lamp_msgs::owner_place::read(&place).map(|place| (place.lat, place.lon)),
        Some((21.04, 105.83))
    );
    // Refused without echoing it, and nothing changed.
    let before = root.files();
    for (line, why) in [
        (
            json!({"owner_place": {"name": "Secret Lane", "lat": 95, "lon": 105.8, "set_at": "2026-10-29T08:30:00Z"}}),
            "lat must be a number from -90 to 90",
        ),
        (
            json!({"owner_place": {"name": "Secret\nLane", "lat": 21.0, "lon": 105.8}}),
            "name must be 1 to 64 characters on one line",
        ),
        (
            json!({"owner_place": "Secret Lane"}),
            "lat must be a number from -90 to 90",
        ),
    ] {
        let (reply, done) = answer(&line.to_string(), &config, true);
        assert_eq!(reply, json!({ "refused": why }));
        assert!(!reply.to_string().contains("Secret"));
        assert!(done.is_none());
    }
    assert_eq!(root.files(), before);
    let (reply, done) = answer(r#"{"owner_place": null}"#, &config, true);
    assert_eq!(reply, json!({"done": {"cleared": true}}));
    assert_eq!(done.unwrap().said, "the owner's place was cleared");
    assert!(!place.exists());
    let (reply, _) = answer(r#"{"owner_place": null}"#, &config, true);
    assert_eq!(reply, json!({"done": {"cleared": true}}));
    // The binary's journal: what changed, never the name or where.
    let (reply, journal) = run(&config, true, &line);
    assert_eq!(reply, json!({ "done": kept }));
    assert_eq!(
        journal.trim(),
        "lamp-setup: change: the owner's place was set"
    );
    let (_, journal) = run(&config, true, r#"{"owner_place": null}"#);
    assert_eq!(
        journal.trim(),
        "lamp-setup: change: the owner's place was cleared"
    );
    // Off, refused as every change is.
    let (reply, _) = answer(&line, &config, false);
    assert_eq!(reply, json!({"refused": "changes after setup are off"}));
    assert!(!place.exists());
}

/// The owner's developer setting (`control.set`): on writes lamp-control's
/// drop-in, folder and all, 0644; off removes it; only a change asks for
/// lamp-control's restart, and an off with none there is no error.
#[test]
fn the_developer_setting_writes_or_removes_lamp_controls_drop_in() {
    let root = Root::new();
    let config = root.config();
    let dropin = &config.control_dropin;
    assert!(!dropin.parent().unwrap().exists());
    // Off with nothing there: no error, nothing to restart.
    let (reply, done) = answer(r#"{"control": false}"#, &config, true);
    assert_eq!(reply, json!({"done": {"on": false}}));
    let done = done.unwrap();
    assert!(!done.restart_control && !done.restart_schedule);
    assert_eq!(done.said, "developer access is off already");
    assert!(!dropin.exists());
    // On: written whole, readable by systemd.
    let (reply, done) = answer(r#"{"control": true}"#, &config, true);
    assert_eq!(reply, json!({"done": {"on": true}}));
    let done = done.unwrap();
    assert!(done.restart_control && !done.restart_schedule);
    assert_eq!(done.said, "developer access is on");
    assert_eq!(
        fs::read_to_string(dropin).unwrap(),
        "[Service]\nEnvironment=LAMP_CONTROL=on\n"
    );
    assert_eq!(
        change::CONTROL_DROPIN_TEXT,
        "[Service]\nEnvironment=LAMP_CONTROL=on\n"
    );
    assert_eq!(mode(dropin), 0o644);
    // No temporary left beside it.
    assert_eq!(fs::read_dir(dropin.parent().unwrap()).unwrap().count(), 1);
    // On again: nothing changed, nothing to restart.
    let (_, done) = answer(r#"{"control": true}"#, &config, true);
    assert!(!done.unwrap().restart_control);
    // A drop-in that says something else is written over.
    fs::write(dropin, "[Service]\nEnvironment=LAMP_CONTROL=off\n").unwrap();
    let (_, done) = answer(r#"{"control": true}"#, &config, true);
    assert!(done.unwrap().restart_control);
    assert_eq!(
        fs::read_to_string(dropin).unwrap(),
        change::CONTROL_DROPIN_TEXT
    );
    // Off: removed, and lamp-control restarted to close the door.
    let (reply, done) = answer(r#"{"control": false}"#, &config, true);
    assert_eq!(reply, json!({"done": {"on": false}}));
    let done = done.unwrap();
    assert!(done.restart_control);
    assert_eq!(done.said, "developer access is off");
    assert!(!dropin.exists());
    // Anything but one boolean is refused, and nothing is written.
    for line in [
        r#"{"control": "on"}"#,
        r#"{"control": 1}"#,
        r#"{"control": null}"#,
    ] {
        let (reply, done) = answer(line, &config, true);
        assert_eq!(
            reply,
            json!({"refused": "control must be true or false"}),
            "{line}"
        );
        assert!(done.is_none());
    }
    let (reply, _) = answer(r#"{"control": true, "x": 1}"#, &config, true);
    assert_eq!(reply, json!({"refused": "the request holds one change"}));
    assert_eq!(
        parse(r#"{"control": true}"#, &config),
        Ok(Change::Control(true))
    );
    // Off, refused as every change is.
    let (reply, _) = answer(r#"{"control": true}"#, &config, false);
    assert_eq!(reply, json!({"refused": "changes after setup are off"}));
    assert!(!dropin.exists());
    // The product's path, and the reset forgets the setting.
    let own: Config =
        serde_json::from_str(include_str!("../../fixtures/serve-own-page.json")).unwrap();
    assert_eq!(
        own.control_dropin,
        std::path::Path::new("/etc/systemd/system/lamp-control.service.d/developer.conf")
    );
    let plan: Value =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    assert!(
        plan["wipe"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == change::CONTROL_DROPIN)
    );
}

#[test]
fn the_lamps_name_in_the_app_is_kept_readable_and_never_empty() {
    let root = Root::new();
    let config = root.config();
    let (reply, done) = answer(r#"{"display_name": "Desk lamp"}"#, &config, true);
    assert_eq!(reply, json!({"done": {"name": "Desk lamp"}}));
    let done = done.unwrap();
    assert!(!done.restart_schedule);
    assert!(!done.said.contains("Desk"), "{}", done.said);
    assert_eq!(
        fs::read_to_string(&config.display_name).unwrap(),
        "Desk lamp\n"
    );
    // lamp-cloud reads it as lampos.
    assert_eq!(mode(&config.display_name), 0o644);
    // The person's name is another file, untouched.
    assert!(!config.name.exists());
    let (reply, _) = answer(r#"{"display_name": "  "}"#, &config, true);
    assert_eq!(reply, json!({"refused": "name must be 1 to 64 characters"}));
    assert_eq!(
        fs::read_to_string(&config.display_name).unwrap(),
        "Desk lamp\n"
    );
}

#[test]
fn a_zone_is_written_as_setup_writes_it_and_only_a_new_one_restarts_schedule() {
    let root = Root::new();
    let config = root.config();
    let (reply, done) = answer(r#"{"timezone": "Europe/Paris"}"#, &config, true);
    assert_eq!(reply, json!({"done": {"timezone": "Europe/Paris"}}));
    let done = done.unwrap();
    assert!(done.restart_schedule);
    assert_eq!(done.said, "the time zone is now Europe/Paris");
    assert_eq!(
        fs::read_link(&config.localtime).unwrap(),
        config.zoneinfo.join("Europe/Paris")
    );
    assert_eq!(
        fs::read_to_string(&config.timezone_file).unwrap(),
        "Europe/Paris\n"
    );
    let env = fs::read_to_string(&config.zone_env).unwrap();
    assert!(env.contains("LAMP_ZONE=Europe/Paris\n"), "{env}");
    assert!(env.contains("LAMP_UTC_OFFSET_MINUTES="), "{env}");
    // The same zone again: nothing for schedule to take up.
    let (_, done) = answer(r#"{"timezone": "Europe/Paris"}"#, &config, true);
    assert!(!done.unwrap().restart_schedule);
    let (_, done) = answer(r#"{"timezone": "Asia/Ho_Chi_Minh"}"#, &config, true);
    assert!(done.unwrap().restart_schedule);
    assert!(
        fs::read_to_string(&config.zone_env)
            .unwrap()
            .contains("LAMP_ZONE=Asia/Ho_Chi_Minh\n")
    );
}

#[test]
fn a_zone_either_zone_data_lacks_is_refused_and_nothing_changes() {
    let root = Root::new();
    let config = root.config();
    answer(r#"{"timezone": "Asia/Ho_Chi_Minh"}"#, &config, true);
    let before = root.files();
    for zone in [
        "Asia/Nowhere",
        "Mars/Olympus",
        "../etc/zone.env",
        "Asia",
        "",
        "UTC+7",
    ] {
        let line = json!({ "timezone": zone }).to_string();
        let (reply, done) = answer(&line, &config, true);
        assert_eq!(
            reply,
            json!({"refused": "timezone must be a zone the lamp knows, such as Asia/Ho_Chi_Minh"}),
            "{zone}"
        );
        assert!(done.is_none());
    }
    assert_eq!(root.files(), before);
}

#[test]
fn off_every_request_is_refused_and_nothing_is_written() {
    let root = Root::new();
    let config = root.config();
    let before = root.files();
    for line in [
        r#"{"person_name": "Lan"}"#,
        r#"{"display_name": "Desk lamp"}"#,
        r#"{"timezone": "Europe/Paris"}"#,
    ] {
        let (reply, done) = answer(line, &config, false);
        assert_eq!(reply, json!({"refused": "changes after setup are off"}));
        assert!(done.is_none());
    }
    assert_eq!(root.files(), before);
}

#[test]
fn a_request_not_exactly_one_change_is_refused_without_echoing_it() {
    let root = Root::new();
    let config = root.config();
    let secret = "Secret Person";
    let long = "x".repeat(65);
    let cases: Vec<(String, &str)> = vec![
        ("not json".into(), "the request is not JSON"),
        ("[1]".into(), "the request is not an object"),
        (
            json!({"person_name": secret, "timezone": "Europe/Paris"}).to_string(),
            "the request holds one change",
        ),
        ("{}".into(), "the request holds one change"),
        (
            json!({"wifi": secret}).to_string(),
            "the request names no change this lamp makes",
        ),
        (json!({"person_name": 7}).to_string(), "name must be text"),
        (
            json!({"person_name": long}).to_string(),
            "name must be at most 64 characters on one line",
        ),
        (
            json!({"display_name": format!("{secret}\nsecond line")}).to_string(),
            "name must be at most 64 characters on one line",
        ),
        (
            json!({"person_name": format!("{secret}\u{202e}")}).to_string(),
            "name must be at most 64 characters on one line",
        ),
        (
            format!(r#"{{"person_name": "{}"}}"#, "y".repeat(5_000)),
            "the request is too long",
        ),
    ];
    let before = root.files();
    for (line, why) in cases {
        let (reply, done) = answer(&line, &config, true);
        assert_eq!(reply, json!({"refused": why}), "{line:.80}");
        assert!(!reply.to_string().contains(secret));
        assert!(done.is_none());
    }
    assert_eq!(root.files(), before);
    // Sixty-four characters fit, as setup's own bound.
    let fits = "ă".repeat(64);
    assert!(matches!(
        parse(&json!({"person_name": fits}).to_string(), &config),
        Ok(Change::PersonName(Some(_)))
    ));
}

/// A voice heard on a clock of the test's own: `voice/state` each second,
/// as the script says for each second.
struct Script {
    now: u64,
    at: Box<dyn Fn(u64) -> Vec<Heard>>,
}

impl Ears for Script {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        self.now += wait.as_millis() as u64;
        (self.at)(self.now)
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
}

const S: u64 = 1_000;

#[test]
fn schedule_waits_for_the_quiet_an_update_waits_for() {
    // Resting all along: 45 s of watching.
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| vec![Heard::State(Voice::Resting)]),
    };
    assert_eq!(wait_for_quiet(&mut ears, change::QUIET_MOST), Ok(()));
    assert_eq!(ears.now, 45 * S);
    // A turn until 20 s: 45 s after its end.
    let mut ears = Script {
        now: 0,
        at: Box::new(|now| {
            if now <= 20 * S {
                vec![Heard::State(Voice::Speaking), Heard::Speech]
            } else {
                vec![Heard::State(Voice::Resting)]
            }
        }),
    };
    assert_eq!(wait_for_quiet(&mut ears, change::QUIET_MOST), Ok(()));
    assert_eq!(ears.now, 65 * S);
    // A conversation that never ends, and a voice that says nothing on the
    // bus: given up at the bound, with why.
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| vec![Heard::State(Voice::Listening)]),
    };
    let why = wait_for_quiet(&mut ears, Duration::from_secs(600)).unwrap_err();
    assert!(why.starts_with("the voice is"), "{why}");
    assert_eq!(ears.now, 600 * S);
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let why = wait_for_quiet(&mut ears, Duration::from_secs(120)).unwrap_err();
    assert!(why.contains("cannot be judged"), "{why}");
}

/// A zone's `lamp-schedule` restart waits for the quiet and then for no
/// update at work, holding lamp-update's lock through the restart: a round
/// puts its build back when a unit it restarted changes process in its
/// watch (`updates.md` step 9). Free, the lock comes after the quiet; held
/// all along, given up at the bound with why; let go meanwhile, taken after
/// the quiet again.
#[test]
fn a_zone_restart_waits_for_no_update_and_holds_its_lock() {
    use std::{cell::RefCell, rc::Rc};
    let folder = std::env::temp_dir().join(format!(
        "lsc-unlocked-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let records = lamp_update::records::Records::new(folder.clone());
    let resting =
        || -> Box<dyn Fn(u64) -> Vec<Heard>> { Box::new(|_| vec![Heard::State(Voice::Resting)]) };
    // No update: the lock after 45 s of quiet, held while the caller acts.
    let mut ears = Script {
        now: 0,
        at: resting(),
    };
    let held = change::quiet_and_unlocked(&mut ears, &records, change::QUIET_MOST).unwrap();
    assert_eq!(ears.now, 45 * S);
    assert!(records.lock().is_err());
    drop(held);
    // An update holding it all along: given up at the bound, with why.
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: resting(),
    };
    let Err(why) = change::quiet_and_unlocked(&mut ears, &records, Duration::from_secs(120)) else {
        panic!("taken while an update held it");
    };
    assert!(why.contains("update"), "{why}");
    assert!(ears.now >= 120 * S, "{}", ears.now);
    // Let go at 70 s: quiet at 45 s, the lock held, looked at again 10 s
    // later after another 45 s of quiet, at 100 s.
    let update = Rc::new(RefCell::new(Some(update)));
    let letting = Rc::clone(&update);
    let mut ears = Script {
        now: 0,
        at: Box::new(move |now| {
            if now >= 70 * S {
                letting.borrow_mut().take();
            }
            vec![Heard::State(Voice::Resting)]
        }),
    };
    let held = change::quiet_and_unlocked(&mut ears, &records, change::QUIET_MOST).unwrap();
    assert_eq!(ears.now, 100 * S);
    assert!(update.borrow().is_none());
    drop(held);
    let _ = fs::remove_dir_all(folder);
}

fn records_folder(name: &str) -> (PathBuf, lamp_update::records::Records) {
    let folder = std::env::temp_dir().join(format!(
        "lsc-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    (folder.clone(), lamp_update::records::Records::new(folder))
}

/// systemd as the test sees it: each call, and whether lamp-update's lock
/// was held when it came; `stop` fails when told to.
struct Units<'a> {
    records: &'a lamp_update::records::Records,
    calls: Vec<(String, bool)>,
    stop_fails: bool,
}

impl change::Systemd for Units<'_> {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String> {
        let held = self.records.lock().is_err();
        self.calls.push((args.join(" "), held));
        if self.stop_fails && args[0] == "stop" {
            return Err("exit status: 1".into());
        }
        Ok(())
    }
}

/// lamp-update's lock alone, no quiet: a voice that never rests does not
/// hold it back; looked for every `every`, given up at the bound.
#[test]
fn control_waits_only_for_the_update_lock() {
    let (folder, records) = records_folder("ctl-unlocked");
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| vec![Heard::State(Voice::Speaking), Heard::Speech]),
    };
    let held = change::unlocked(
        &mut ears,
        &records,
        change::UPDATE_LOOK_EVERY,
        change::QUIET_MOST,
    )
    .unwrap();
    assert_eq!(ears.now, 0);
    drop(held);
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let Err(why) = change::unlocked(&mut ears, &records, S_1, Duration::from_secs(8)) else {
        panic!("taken while an update held it");
    };
    assert!(why.contains("update"), "{why}");
    assert_eq!(ears.now, 8 * S);
    drop(update);
    let _ = fs::remove_dir_all(folder);
}

const S_1: Duration = Duration::from_secs(1);

/// Off with the drop-in there: lamp-update's lock taken, the drop-in
/// removed, reload and stop under it, and only then the reply (returned
/// once they ran); a stop that fails is said, never claimed closed. Off
/// with none there is answered as any change, with no systemctl.
#[test]
fn off_holds_the_update_lock_and_stops_before_the_reply() {
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-off");
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let off = r#"{"control": false}"#;
    // None there: not this path.
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    assert!(
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut units).is_none()
    );
    assert!(units.calls.is_empty());
    answer(r#"{"control": true}"#, &config, true);
    // Without --enable, not this path either.
    assert!(
        change::close_before_answer(off, &config, false, &records, &mut ears, &mut units).is_none()
    );
    // On isn't this path.
    assert!(
        change::close_before_answer(
            r#"{"control": true}"#,
            &config,
            true,
            &records,
            &mut ears,
            &mut units
        )
        .is_none()
    );
    let (reply, done) =
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut units).unwrap();
    // The reply exists only now, after both calls, each under the lock.
    assert_eq!(
        units.calls,
        vec![
            ("daemon-reload".to_owned(), true),
            ("stop lamp-control.service".to_owned(), true)
        ]
    );
    assert_eq!(reply, json!({"done": {"on": false, "closed": true}}));
    let done = done.unwrap();
    assert!(!done.restart_control && !done.restart_schedule);
    assert!(!config.control_dropin.exists());
    // Let go once answered.
    drop(records.lock().unwrap());
    // A stop that fails: still answered, saying the door may stay open.
    answer(r#"{"control": true}"#, &config, true);
    let mut failing = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: true,
    };
    let (reply, _) =
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut failing).unwrap();
    assert_eq!(
        reply,
        json!({"done": {"on": false, "closed": false, "note": change::CLOSE_MAY_STAY_OPEN}})
    );
    assert!(!config.control_dropin.exists());
    let _ = fs::remove_dir_all(folder);
}

/// Off while an update holds the lock past about 8 s: refused, the drop-in
/// left in place, systemd not asked.
#[test]
fn off_while_an_update_holds_the_lock_refuses_and_keeps_the_drop_in() {
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-busy");
    answer(r#"{"control": true}"#, &config, true);
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    let (reply, done) = change::close_before_answer(
        r#"{"control": false}"#,
        &config,
        true,
        &records,
        &mut ears,
        &mut units,
    )
    .unwrap();
    assert_eq!(reply, json!({"refused": change::CLOSE_INSTALLING}));
    assert_eq!(
        change::CLOSE_INSTALLING,
        "an update is installing: try again in a few minutes"
    );
    assert!(done.is_none());
    assert!(units.calls.is_empty());
    assert_eq!(ears.now, 8 * S);
    assert_eq!(
        fs::read_to_string(&config.control_dropin).unwrap(),
        change::CONTROL_DROPIN_TEXT
    );
    drop(update);
    let _ = fs::remove_dir_all(folder);
}

/// On: answered first (the drop-in written), then lamp-update's lock, with
/// no quiet, and reload and restart under it once an update lets it go.
#[test]
fn on_replies_then_restarts_once_unlocked() {
    use std::{cell::RefCell, rc::Rc};
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-on");
    let (reply, done) = answer(r#"{"control": true}"#, &config, true);
    assert_eq!(reply, json!({"done": {"on": true}}));
    assert!(done.unwrap().restart_control);
    assert!(config.control_dropin.exists());
    // An update holds the lock until 25 s; the voice never rests.
    let update = Rc::new(RefCell::new(Some(records.lock().unwrap())));
    let letting = Rc::clone(&update);
    let mut ears = Script {
        now: 0,
        at: Box::new(move |now| {
            if now >= 25 * S {
                letting.borrow_mut().take();
            }
            vec![Heard::State(Voice::Speaking)]
        }),
    };
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    let said = change::restart_control_when_unlocked(&mut ears, &records, &mut units);
    assert_eq!(said, "lamp-control restarted on the developer setting");
    // Looked at every 10 s: free at 30 s.
    assert_eq!(ears.now, 30 * S);
    assert_eq!(
        units.calls,
        vec![
            ("daemon-reload".to_owned(), true),
            ("restart lamp-control.service".to_owned(), true)
        ]
    );
    let _ = fs::remove_dir_all(folder);
}

/// The app's restart and shutdown (#2193, #3697): off, refused as every
/// change is; on, answered first, holding lamp-update's lock to act on;
/// refused with why while an update holds it, and nothing to act on. A
/// line that asks neither is a change, not a power request.
#[test]
fn a_restart_or_shutdown_is_answered_first_and_refused_under_an_update() {
    use lamp_keeper::restart::{INSTALLING, REQUEST, SHUTDOWN_REQUEST};
    let folder = std::env::temp_dir().join(format!(
        "lsc-power-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let records = lamp_update::records::Records::new(folder.clone());
    assert!(change::power(r#"{"person_name":"Lan"}"#, true, &records).is_none());
    assert!(change::power(r#"{"shutdown":"machine"}"#, true, &records).is_none());
    for (line, power, done) in [
        (REQUEST, change::Power::Restart, json!({"restarting": true})),
        (
            SHUTDOWN_REQUEST,
            change::Power::Shutdown,
            json!({"shutting_down": true}),
        ),
    ] {
        let (asked, reply, lock) = change::power(line, false, &records).unwrap();
        assert_eq!(asked, power);
        assert_eq!(reply, json!({"refused": change::OFF}));
        assert!(lock.is_none());
        let held = records.lock().unwrap();
        let (_, reply, lock) = change::power(line, true, &records).unwrap();
        assert_eq!(reply, json!({ "refused": INSTALLING }), "{line}");
        assert!(lock.is_none());
        drop(held);
        let (_, reply, lock) = change::power(line, true, &records).unwrap();
        assert_eq!(reply, json!({ "done": done }), "{line}");
        // Held while it acts: an update that starts meanwhile can't have it.
        assert!(lock.is_some());
        assert!(records.lock().is_err());
    }
    let _ = fs::remove_dir_all(folder);
}

/// A restart from the app waits for the voice to rest between turns (5 s),
/// never cutting a turn, and restarts all the same after 45 s.
#[test]
fn a_restart_from_the_app_waits_for_the_voice_to_rest() {
    // A turn until 20 s: restarted 5 s after it ends.
    let mut ears = Script {
        now: 0,
        at: Box::new(|now| {
            if now <= 20 * S {
                vec![Heard::State(Voice::Speaking), Heard::Speech]
            } else {
                vec![Heard::State(Voice::Resting)]
            }
        }),
    };
    let mut at = None;
    let said = change::restart_when_rested(&mut ears, || {
        at = Some(());
        Ok(())
    });
    assert!(at.is_some());
    assert_eq!(ears.now, 25 * S);
    assert_eq!(said, "the voice rested; lampos.target restarted");
    // A turn that never ends: restarted at 45 s all the same, and said so.
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| vec![Heard::State(Voice::Listening)]),
    };
    let said = change::restart_when_rested(&mut ears, || Err("exit status: 1".into()));
    assert_eq!(ears.now, 45 * S);
    assert!(
        said.starts_with("the voice did not rest within 45 s (the voice is"),
        "{said}"
    );
    assert!(
        said.ends_with("; lampos.target could not be restarted: exit status: 1"),
        "{said}"
    );
}

/// The binary as its unit runs it: one line in on standard input, one
/// line out, a journal line that never holds the name.
fn run(config: &Config, enable: bool, line: &str) -> (Value, String) {
    let file = config
        .name
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("serve.json");
    fs::write(&file, serde_json::to_vec(config).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-setup"));
    command.args(["change", "--config"]).arg(&file);
    if enable {
        command.arg("--enable");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    (
        serde_json::from_str(&stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// On the socket systemd hands over, only lamp-cloud may ask, as for the
/// keys: a peer that isn't it (here, this test, outside
/// `lamp-cloud.service`) is refused before its line is read, and nothing
/// is changed. Standard input that isn't a socket is the helper run by hand
/// (the test above).
#[test]
fn a_socket_peer_that_is_not_lamp_cloud_is_refused_and_nothing_changes() {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    let root = Root::new();
    let config = root.config();
    let file = config
        .name
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("serve.json");
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let before = fs::read_to_string(&config.name).ok();
    for helper in ["change", "wifi"] {
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-setup"))
            .args([helper, "--config"])
            .arg(&file)
            .arg("--enable")
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ours.write_all(b"{\"person_name\": \"Lan\"}\n").unwrap();
        drop(ours);
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{helper}");
        let reply: Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(
            reply,
            json!({"refused": "only the app's link may change the lamp's settings"}),
            "{helper}"
        );
        let journal = String::from_utf8(output.stderr).unwrap();
        assert!(
            journal.contains("refused: the peer is not lamp-cloud.service"),
            "{helper}: {journal}"
        );
        assert!(!journal.contains("Lan"), "{journal}");
    }
    assert_eq!(
        fs::read_to_string(&config.name).ok(),
        before,
        "nothing changed"
    );
}

#[test]
fn the_helper_answers_one_line_and_never_journals_a_name() {
    let root = Root::new();
    let config = root.config();
    let (reply, journal) = run(&config, true, r#"{"person_name": "Minh Anh"}"#);
    assert_eq!(reply, json!({"done": {"name": "Minh Anh"}}));
    assert_eq!(
        journal.trim(),
        "lamp-setup: change: the person's name was changed (8 characters)"
    );
    let (reply, journal) = run(&config, true, r#"{"display_name": "Bàn học"}"#);
    assert_eq!(reply["done"]["name"], "Bàn học");
    assert!(!journal.contains("Bàn"), "{journal}");
    let (reply, journal) = run(&config, true, r#"{"person_name": "Lan", "x": 1}"#);
    assert_eq!(reply, json!({"refused": "the request holds one change"}));
    assert!(!journal.contains("Lan"), "{journal}");
    // Without --enable: refused, nothing written.
    let (reply, _) = run(&config, false, r#"{"person_name": "Lan"}"#);
    assert_eq!(reply, json!({"refused": "changes after setup are off"}));
    assert_eq!(fs::read_to_string(&config.name).unwrap(), "Minh Anh\n");
}

#[test]
fn the_products_reset_wipes_the_lamps_name_and_the_unit_runs_the_helper() {
    let plan: Value =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    let wiped = |path: &str| plan["wipe"].as_array().unwrap().iter().any(|w| w == path);
    assert!(wiped("/etc/lampOS/setup/display-name"));
    assert!(wiped("/etc/lampOS/person/name"));
    // The owner's place, beside the name (#4333).
    assert!(wiped("/etc/lampOS/person/place"));
    let own: Config =
        serde_json::from_str(include_str!("../../fixtures/serve-own-page.json")).unwrap();
    assert_eq!(
        lamp_msgs::owner_place::beside_name(&own.name),
        std::path::Path::new(lamp_msgs::owner_place::FILE)
    );
    assert_eq!(
        own.display_name,
        std::path::Path::new("/etc/lampOS/setup/display-name")
    );
    let unit = include_str!("../../../../deploy/product/lamp-setup-change@.service");
    assert!(unit.contains(
        "ExecStart=/opt/lampOS/setup/current/lamp-setup change --config /etc/lampOS/setup/serve.json --bus-root /run/lampos-bus --enable\n"
    ));
    assert!(unit.contains("StandardInput=socket\nStandardOutput=socket\n"));
    let socket = include_str!("../../../../deploy/product/lamp-setup-change.socket");
    assert!(socket.contains("ListenStream=/run/lampos-setup-change/change.sock\n"));
    assert!(socket.contains("SocketGroup=lampos\nSocketMode=0660\n"));
    assert!(socket.contains("Accept=yes\n"));
}
