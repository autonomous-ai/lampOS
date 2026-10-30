//! `lamp-update install` onto a blank lamp made of files, with a channel
//! and a machine of the test's own: no network, no systemd, no lamp
//! (docs/install.md, task #811).
use lamp_update::{
    fetch::{Channel, Fetch},
    install::{self, Host, Install, PRODUCT, SETTINGS},
    manifest::{self, File, Manifest, sha256_hex},
    records::{Records, State},
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const CHANNEL: &str = "https://channel.test/lamp-voice/";

#[derive(Default)]
struct FakeChannel {
    files: HashMap<String, Vec<u8>>,
}

impl Channel for FakeChannel {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        let bytes = self.files.get(url).ok_or(Fetch::Missing)?;
        if bytes.len() as u64 > most {
            return Err(Fetch::Failed("larger than expected".into()));
        }
        out.write_all(bytes).unwrap();
        Ok(bytes.len() as u64)
    }
}

/// The machine: users go into the root's /etc/passwd, and every call is
/// remembered.
struct FakeHost {
    root: PathBuf,
    calls: Vec<String>,
    /// How many of the next `enable`s fail, as a `systemctl` that is not
    /// there yet or refuses.
    failing_enables: u32,
    /// `harness service install` refuses, as for a user it cannot serve.
    harness_refuses: bool,
    /// Each command `harness-off`'s steps ran, as its arguments.
    steps: Vec<String>,
    /// Each command Harness at boot's steps ran, as its arguments.
    boot_steps: Vec<String>,
    /// The step that fails: the first command containing this text.
    failing_step: Option<String>,
    /// What `harness service status --json` prints.
    status_json: String,
    /// What `pgrep` finds; none makes it end with 1, as `pgrep` does.
    pids: String,
    /// What each `harness version` says, in turn; when none is left, the
    /// version `harness-on` needs.
    versions: std::collections::VecDeque<Result<String, String>>,
    /// The sha256 of the lamp-update running install: the build's own
    /// unless a test says otherwise (#3733).
    running: Result<String, String>,
    /// Whether this run was handed over to already (#3733).
    handed_over: bool,
}

/// `harness service status --json` once the service is gone (Harness
/// 0.4.17's fields, `cli/src/lib/serviceCommand.ts`).
const STATUS_GONE: &str = r#"{"supported":true,"platform":"systemd","file":"/x","target":"/y","installed":false,"current":false,"registered":false,"running":false,"pid":null,"state":"inactive","lastExit":null,"failed":false,"linger":true,"unreachable":null}"#;

impl Host for FakeHost {
    fn add_user(&mut self, name: &str, groups: &[String]) -> Result<(), String> {
        let passwd = self.root.join("etc/passwd");
        let mut text = fs::read_to_string(&passwd).unwrap_or_default();
        text.push_str(&format!(
            "{name}:x:999:999::/nonexistent:/usr/sbin/nologin\n"
        ));
        fs::write(passwd, text).unwrap();
        self.calls
            .push(format!("useradd {name} {}", groups.join(",")));
        Ok(())
    }

    fn enable(&mut self, units: &[String]) -> Result<(), String> {
        self.calls.push(format!("enable {}", units.join(" ")));
        if self.failing_enables > 0 {
            self.failing_enables -= 1;
            return Err("systemctl enable: Failed to connect to bus".into());
        }
        Ok(())
    }

    fn tmpfiles(&mut self, path: &str) -> Result<(), String> {
        self.calls.push(format!("tmpfiles {path}"));
        Ok(())
    }

    fn harness_at_boot(&mut self, user: &str, home: &str) -> Result<(), String> {
        self.calls.push(format!("harness {user} {home}"));
        // The real steps, each through stand-ins, the drop-in written under
        // the root; `harness service install` refuses on demand.
        let (steps, refuses) = (&mut self.boot_steps, self.harness_refuses);
        install::harness_at_boot_with(
            &mut |argv: &[String]| {
                let line = argv.join(" ");
                steps.push(line.clone());
                match argv[0].as_str() {
                    "id" => Ok("999\n".into()),
                    _ if refuses && line.ends_with("service install") => {
                        Err("harness service install: no user manager".into())
                    }
                    _ => Ok(String::new()),
                }
            },
            &self.root,
            user,
            home,
        )?;
        // What logind and the Harness CLI leave behind.
        let linger = self.root.join("var/lib/systemd/linger");
        fs::create_dir_all(&linger).unwrap();
        fs::write(linger.join(user), "").unwrap();
        let unit = self
            .root
            .join(home.trim_start_matches('/'))
            .join(".config/systemd/user");
        fs::create_dir_all(&unit).unwrap();
        fs::write(unit.join("harnessd.service"), "[Service]\n").unwrap();
        Ok(())
    }

    fn harness_on(&mut self, user: &str, home: &str) -> Result<(), String> {
        self.calls.push(format!("harness-on {user} {home}"));
        // The real steps, each through stand-ins; the link is what `ln`
        // leaves, a file at HARNESS here.
        let (steps, failing, root) = (
            &mut self.steps,
            self.failing_step.clone(),
            self.root.clone(),
        );
        install::harness_on_with(
            &mut |argv: &[String]| {
                let line = argv.join(" ");
                steps.push(line.clone());
                if failing.as_deref().is_some_and(|step| line.contains(step)) {
                    return Err(format!("{line}: refused"));
                }
                match argv[0].as_str() {
                    "id" => Ok("999\n".into()),
                    "ln" => {
                        let link = root.join(argv[3].trim_start_matches('/'));
                        fs::create_dir_all(link.parent().unwrap()).unwrap();
                        // Never written through a link already there.
                        let _ = fs::remove_file(&link);
                        fs::write(link, "#!/bin/sh\n").unwrap();
                        Ok(String::new())
                    }
                    _ => Ok(String::new()),
                }
            },
            user,
            home,
        )
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        self.calls.push(format!("restart {unit}"));
        Ok(())
    }

    fn harness_version(&mut self, user: &str, home: &str) -> Result<String, String> {
        self.calls.push(format!("harness version {user} {home}"));
        self.versions
            .pop_front()
            .unwrap_or_else(|| Ok("0.4.23\n".into()))
    }

    fn harness_off(&mut self, user: &str, home: &str) -> Result<Vec<String>, String> {
        self.calls.push(format!("harness-off {user} {home}"));
        // The real steps, each through stand-ins for `id`, `runuser … harness`,
        // `loginctl` and `pgrep`.
        let (steps, failing) = (&mut self.steps, self.failing_step.clone());
        let (status, pids) = (self.status_json.clone(), self.pids.clone());
        let root = self.root.clone();
        let left = install::harness_off_with(
            &mut |argv: &[String]| {
                let line = argv.join(" ");
                steps.push(line.clone());
                if failing.as_deref().is_some_and(|step| line.contains(step)) {
                    return Err(format!("{line}: refused"));
                }
                match argv[0].as_str() {
                    "id" => Ok("999\n".into()),
                    "rm" => {
                        let _ = fs::remove_file(root.join(argv[2].trim_start_matches('/')));
                        Ok(String::new())
                    }
                    "pgrep" if pids.is_empty() => Err("pgrep: exit 1".into()),
                    "pgrep" => Ok(pids.clone()),
                    _ if line.ends_with("service status --json") => Ok(status.clone()),
                    _ => Ok(String::new()),
                }
            },
            user,
            home,
        )?;
        // What `harness_at_boot` left, gone: the CLI's unit and the linger.
        let _ = fs::remove_file(self.root.join("var/lib/systemd/linger").join(user));
        let _ = fs::remove_file(
            self.root
                .join(home.trim_start_matches('/'))
                .join(".config/systemd/user/harnessd.service"),
        );
        Ok(left)
    }

    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String> {
        self.calls.push(format!("offset {zone}"));
        Ok(if zone == "Asia/Ho_Chi_Minh" { 420 } else { 0 })
    }

    fn running_sha256(&mut self) -> Result<String, String> {
        self.running.clone()
    }

    fn handed_over(&mut self) -> bool {
        self.handed_over
    }

    fn hand_over(&mut self, exe: &std::path::Path) -> String {
        // A real hand-over never comes back; here it is only recorded.
        self.calls.push(format!("hand over {}", exe.display()));
        "faked".into()
    }
}

struct Lamp {
    _dir: tempfile::TempDir,
    root: PathBuf,
    channel: FakeChannel,
    host: FakeHost,
    config: Install,
    said: Vec<String>,
}

/// Every binary the product starts, the voice's worker and the body's
/// factory tools: a line of text each.
fn binaries() -> Vec<String> {
    let mut names: Vec<String> = install::parts()
        .into_iter()
        .map(|part| part.binary)
        .collect();
    names.push("lamp-turn-end".into());
    names.push("lamp-body-measure".into());
    names.push("lamp-body-mood".into());
    names
}

impl Lamp {
    /// A blank lamp: the base image's /etc, its zone and the update key.
    /// The channel carries `build` with every binary but those in `leave_out`.
    fn new(sequence: u64, leave_out: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(root.join("etc/passwd"), "root:x:0:0::/root:/bin/bash\n").unwrap();
        fs::write(
            root.join("etc/group"),
            "root:x:0:\naudio:x:29:\nvideo:x:44:\ndialout:x:20:\n",
        )
        .unwrap();
        fs::write(root.join("etc/timezone"), "Asia/Ho_Chi_Minh\n").unwrap();
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let mut channel = FakeChannel::default();
        let mut files = Vec::new();
        let mut running = Err("no lamp-update in the build".to_owned());
        for name in binaries() {
            if leave_out.contains(&name.as_str()) {
                continue;
            }
            let bytes = format!("{name} of build {sequence}\n").into_bytes();
            if name == "lamp-update" {
                running = Ok(sha256_hex(&bytes));
            }
            files.push(File {
                name: name.clone(),
                sha256: sha256_hex(&bytes),
                size: bytes.len() as u64,
            });
            channel.files.insert(format!("{CHANNEL}{name}"), bytes);
        }
        let manifest = Manifest {
            schema: 1,
            channel: "lamp-voice".into(),
            sequence,
            commit: "a".repeat(40),
            files_url: CHANNEL.into(),
            files,
            units: Vec::new(),
            min_updater: None,
            models: Vec::new(),
            settings: Vec::new(),
            image: Vec::new(),
            reset_plan: None,
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        channel.files.insert(
            format!("{CHANNEL}{}", manifest::ENVELOPE),
            manifest::seal(&bytes, &key).unwrap(),
        );
        let config = Install {
            root: root.clone(),
            channel: "lamp-voice".into(),
            channel_url: CHANNEL.into(),
            key: key.public_key().as_ref().try_into().unwrap(),
            records: "/var/lib/lampos-update/".into(),
            setup: false,
            models: Vec::new(),
        };
        let host = FakeHost {
            root: root.clone(),
            calls: Vec::new(),
            failing_enables: 0,
            harness_refuses: false,
            steps: Vec::new(),
            boot_steps: Vec::new(),
            failing_step: None,
            status_json: STATUS_GONE.into(),
            pids: String::new(),
            versions: Default::default(),
            running,
            handed_over: false,
        };
        Self {
            _dir: dir,
            root,
            channel,
            host,
            config,
            said: Vec::new(),
        }
    }

    fn install(&mut self) -> Result<install::Installed, String> {
        let said = &mut self.said;
        install::install(
            &self.config,
            &mut self.channel,
            &mut self.host,
            &mut |line: &str| said.push(line.to_owned()),
        )
    }

    fn on(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
}

/// Every file and link under a folder, with its bytes or its target.
fn tree(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let key = path.strip_prefix(root).unwrap().to_path_buf();
            if meta.file_type().is_symlink() {
                found.insert(
                    key,
                    format!("-> {}", fs::read_link(&path).unwrap().display()),
                );
            } else if meta.is_dir() {
                folders.push(path);
            } else {
                found.insert(key, sha256_hex(&fs::read(&path).unwrap()));
            }
        }
    }
    found
}

#[test]
fn a_blank_lamp_gets_every_part_its_units_and_nothing_started() {
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = Lamp::new(188, &[]);
    let done = lamp.install().unwrap();
    assert_eq!(done.sequence, 188);

    for part in install::parts() {
        let current = lamp.on(&format!("{}current", part.folder));
        let target = fs::read_link(&current).unwrap();
        assert_eq!(
            target.to_string_lossy().len(),
            64,
            "{current:?} -> {target:?}"
        );
        let binary = fs::read_to_string(current.join(&part.binary)).unwrap();
        assert_eq!(binary, format!("{} of build 188\n", part.binary));
    }
    // The voice's worker beside the voice, where the voice starts it.
    assert!(lamp.on("/opt/lampOS/voice/current/lamp-turn-end").is_file());
    // The body's factory tools beside the body, where steps.sh runs them (#2189).
    for tool in ["lamp-body-measure", "lamp-body-mood"] {
        assert!(
            lamp.on(&format!("/opt/lampOS/body/current/{tool}"))
                .is_file(),
            "{tool}"
        );
    }

    for (name, text) in PRODUCT {
        let written = fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{name}"))).unwrap();
        assert_eq!(&written, text, "{name}");
        let mode = fs::metadata(lamp.on(&format!("/etc/systemd/system/{name}")))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644, "{name}");
    }
    assert!(lamp.on("/etc/tmpfiles.d/lampos.conf").is_file());
    assert!(lamp.on("/etc/lampOS/controls/controls-pins.json").is_file());
    // Records' bound, or its timer runs nothing on a product lamp.
    assert!(lamp.on("/etc/lampOS/records.json").is_file());
    assert!(lamp.on("/etc/lampOS/expression/expression.json").is_file());
    assert_eq!(
        fs::read_to_string(lamp.on("/etc/lampOS/zone.env"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>(),
        ["LAMP_ZONE=Asia/Ho_Chi_Minh", "LAMP_UTC_OFFSET_MINUTES=420"]
    );

    // Users, the tmpfiles, enabled; never started, and setup's marker never written.
    let calls = lamp.host.calls.join("\n");
    assert!(
        calls.contains("useradd lampos audio,video,dialout"),
        "{calls}"
    );
    assert!(calls.contains("useradd lampos-memory lampos"), "{calls}");
    assert!(
        calls.contains("tmpfiles /etc/tmpfiles.d/lampos.conf"),
        "{calls}"
    );
    let enabled = lamp
        .host
        .calls
        .iter()
        .find(|call| call.starts_with("enable "))
        .unwrap();
    for name in [
        "lampos.target",
        "lamp-voice.service",
        "lamp-update.service",
        "lamp-records.timer",
    ] {
        assert!(
            enabled.split(' ').any(|word| word == name),
            "{name} in {enabled}"
        );
    }
    assert!(
        !enabled.contains("lamp-records.service"),
        "the timer starts records"
    );
    assert!(!lamp.on("/var/lib/lampos-setup/done").exists());
    // What is one lamp's own is not written, and the person is told.
    assert!(!lamp.on("/etc/lampOS/body/lamp.env").exists());
    assert!(
        lamp.said.iter().any(|line| line.contains("body/lamp.env")),
        "{:?}",
        lamp.said
    );
    assert!(
        lamp.said.iter().any(|line| line.contains("no group i2c")),
        "{:?}",
        lamp.said
    );

    // lamp-update knows which build the lamp runs.
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.on("/var/lib/lampos-update/state.json")).unwrap())
            .unwrap();
    assert_eq!(state["installed"]["sequence"], 188, "{state}");
}

/// The lines install said about the lamp-update running it (#3733).
fn updater_said(lamp: &Lamp) -> Vec<&str> {
    lamp.said
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("this lamp-update"))
        .collect()
}

/// The hand-overs install asked of the host (#3733).
fn handed(lamp: &Lamp) -> Vec<&str> {
    lamp.host
        .calls
        .iter()
        .map(String::as_str)
        .filter(|call| call.starts_with("hand over "))
        .collect()
}

#[test]
fn another_lamp_update_hands_over_to_the_builds_own_before_any_unit() {
    let mut lamp = Lamp::new(188, &[]);
    let builds = lamp.host.running.clone().unwrap();
    lamp.host.running = Ok(sha256_hex(b"lamp-update of build 120\n"));
    lamp.install().unwrap();
    let own = format!("/opt/lampOS/update/{builds}/lamp-update");
    let placed = lamp.on(&own);
    assert_eq!(
        handed(&lamp),
        [format!("hand over {}", placed.display())],
        "{:?}",
        lamp.host.calls
    );
    // The file handed over to is the build's own, placed before the units.
    assert_eq!(sha256_hex(&fs::read(&placed).unwrap()), builds);
    let at = |prefix: &str| {
        lamp.host
            .calls
            .iter()
            .position(|call| call.starts_with(prefix))
    };
    assert!(at("hand over ") < at("enable "), "{:?}", lamp.host.calls);
    assert!(
        lamp.said.iter().any(|line| line
            == &format!("handing over to build 188's own lamp-update, {own}, to write its units")),
        "{:?}",
        lamp.said
    );
    // A hand-over that comes back is said, and install writes its own.
    assert!(
        lamp.said
            .iter()
            .any(|line| line == "could not hand over: faked"),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_run_handed_over_to_already_hands_over_no_further() {
    // Thirteen's P3 on #4162: the channel moved on between the two runs.
    let mut lamp = Lamp::new(188, &[]);
    lamp.host.running = Ok(sha256_hex(b"lamp-update of build 187\n"));
    lamp.host.handed_over = true;
    lamp.install().unwrap();
    assert_eq!(handed(&lamp), Vec::<&str>::new());
    assert!(
        lamp.said.iter().any(|line| line
            == "this lamp-update was handed this install already, so it writes its own \
                units over build 188's binaries"),
        "{:?}",
        lamp.said
    );
    assert!(
        !lamp.said.iter().any(|line| line.starts_with("handing over")
            || line.starts_with("could not hand over")),
        "{:?}",
        lamp.said
    );
    // Then #4085's warning, once.
    let warned = updater_said(&lamp)
        .into_iter()
        .filter(|line| line.starts_with("this lamp-update (sha256 "))
        .count();
    assert_eq!(warned, 1, "{:?}", lamp.said);
}

#[test]
fn a_lamp_update_not_the_builds_own_is_named_and_install_still_writes_the_units() {
    let mut lamp = Lamp::new(188, &[]);
    let builds = lamp.host.running.clone().unwrap();
    let older = sha256_hex(b"lamp-update of build 120\n");
    lamp.host.running = Ok(older.clone());
    let done = lamp.install().unwrap();
    assert_eq!(done.sequence, 188);
    assert_eq!(
        updater_said(&lamp),
        [format!(
            "this lamp-update (sha256 {}) is not build 188's ({}): the units written are \
                 this lamp-update's own, not build 188's; run build 188's own, \
                 /opt/lampOS/update/{builds}/lamp-update install, to write its units",
            &older[..12],
            &builds[..12]
        )],
        "{:?}",
        lamp.said
    );
    // Said, never refused: the units and the state are written as ever.
    for (name, text) in PRODUCT {
        let written = fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{name}"))).unwrap();
        assert_eq!(&written, text, "{name}");
    }
    assert!(
        lamp.host
            .calls
            .iter()
            .any(|call| call.starts_with("enable "))
    );
}

#[test]
fn the_builds_own_lamp_update_says_nothing_of_itself() {
    let mut lamp = Lamp::new(188, &[]);
    // The same hash in capitals is the same file.
    lamp.host.running = Ok(lamp.host.running.clone().unwrap().to_uppercase());
    lamp.install().unwrap();
    assert_eq!(updater_said(&lamp), Vec::<&str>::new(), "{:?}", lamp.said);
    assert_eq!(handed(&lamp), Vec::<&str>::new());
}

#[test]
fn a_lamp_update_that_cannot_be_read_says_it_could_not_be_checked() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.host.running = Err("/proc/self/exe: Permission denied".into());
    lamp.install().unwrap();
    assert_eq!(
        updater_said(&lamp),
        [
            "this lamp-update could not be checked against build 188's: \
              /proc/self/exe: Permission denied"
        ],
        "{:?}",
        lamp.said
    );
    // Not known to be another: no hand-over.
    assert_eq!(handed(&lamp), Vec::<&str>::new());
}

#[test]
fn an_install_that_ends_before_its_units_says_nothing_of_the_lamp_update() {
    let mut lamp = Lamp::new(188, &["lamp-voice"]);
    lamp.host.running = Ok(sha256_hex(b"lamp-update of build 120\n"));
    let why = lamp.install().unwrap_err();
    assert!(why.starts_with(install::UNITS_NOT_WRITTEN), "{why}");
    assert_eq!(updater_said(&lamp), Vec::<&str>::new(), "{:?}", lamp.said);
    assert_eq!(handed(&lamp), Vec::<&str>::new());
}

#[test]
fn a_build_without_a_lamp_update_has_nothing_to_check_against() {
    // The Reviewer-lead's P3 on #4085: an unreadable running file says
    // nothing when the build carries no lamp-update to check it against.
    let manifest = Manifest {
        schema: 1,
        channel: "lamp-voice".into(),
        sequence: 188,
        commit: "a".repeat(40),
        files_url: CHANNEL.into(),
        files: Vec::new(),
        units: Vec::new(),
        min_updater: None,
        models: Vec::new(),
        settings: Vec::new(),
        image: Vec::new(),
        reset_plan: None,
    };
    let running = Err("/proc/self/exe: Permission denied".to_owned());
    assert_eq!(install::not_the_builds_updater(&manifest, running), None);
    let running = Ok(sha256_hex(b"lamp-update of build 120\n"));
    assert_eq!(install::not_the_builds_updater(&manifest, running), None);
}

#[test]
fn a_closed_etc_lampos_is_said_and_left_as_it_is() {
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = Lamp::new(188, &[]);
    fs::create_dir_all(lamp.on("/etc/lampOS")).unwrap();
    fs::set_permissions(lamp.on("/etc/lampOS"), fs::Permissions::from_mode(0o700)).unwrap();
    lamp.install().unwrap();
    let mode = fs::metadata(lamp.on("/etc/lampOS"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("/etc/lampOS is closed")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn run_twice_it_changes_nothing() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let before = tree(&lamp.root);
    let calls = lamp.host.calls.len();
    let again = lamp.install().unwrap();
    assert_eq!(again.changed, Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);
    assert_eq!(lamp.host.calls.len(), calls, "nothing asked of the machine");
}

#[test]
fn the_exit_note_allowlist_is_exactly_clouds_product_services() {
    // Cloud already uses update in its tests; read its literal allowlist
    // without introducing the reverse crate dependency just for this check.
    let source = include_str!("../../cloud/src/reports.rs");
    let list = source
        .split_once("pub const UNITS: &[&str] = &[")
        .unwrap()
        .1
        .split_once("];")
        .unwrap()
        .0;
    let names: Vec<&str> = list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix('"').unwrap().strip_suffix("\",").unwrap())
        .collect();
    assert_eq!(install::EXIT_NOTE_UNITS, names);
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut services: Vec<_> = fs::read_dir(product)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("lamp-") && name.ends_with(".service"))
        .collect();
    services.sort();
    assert_eq!(services, names, "no duplicate, omitted or extra unit");
    assert_eq!(
        section(install::EXIT_NOTE, "Service"),
        ["ExecStopPost=-+/opt/lampOS/cloud/current/lamp-cloud exit-note %n"]
    );
}

#[test]
fn install_prepares_store_off_and_keeps_the_lamps_store_choice() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let env = "/etc/lampOS/skills/store.env";
    let drop_in = "/etc/systemd/system/lamp-skills-manage@.service.d/store.conf";
    for (path, text) in install::STORE_SETTINGS {
        assert_eq!(fs::read_to_string(lamp.on(path)).unwrap(), *text);
        assert_eq!(mode_of(&lamp.on(path)), 0o644);
        assert_eq!(lamp.state().settings[*path], sha256_hex(text.as_bytes()));
    }
    assert!(
        fs::read_to_string(lamp.on(env))
            .unwrap()
            .lines()
            .any(|l| l == "LAMP_SKILLS_STORE=off")
    );
    assert!(!lamp.on(lamp_skills::package::STORE_KEY).exists());
    for unit in ["lamp-cloud.service", "lamp-skills-manage@.service"] {
        let text = fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{unit}"))).unwrap();
        assert!(section(&text, "Service").contains(&"Environment=LAMP_SKILLS_STORE=off"));
    }
    let helper =
        fs::read_to_string(lamp.on("/etc/systemd/system/lamp-skills-manage@.service")).unwrap();
    assert!(helper.contains(&format!("--key-file {}", lamp_skills::package::STORE_KEY)));
    assert!(!helper.contains("/etc/lampOS/update/key.pub"));
    let cloud = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-cloud.service")).unwrap();
    let helper_drop_in = fs::read_to_string(lamp.on(drop_in)).unwrap();
    for text in [&cloud, &helper_drop_in] {
        assert!(
            section(text, "Service").contains(&"EnvironmentFile=-/etc/lampOS/skills/store.env")
        );
    }
    assert!(section(&helper_drop_in, "Service").contains(&"PrivateNetwork=no"));
    assert!(lamp.install().unwrap().changed.is_empty());
    // A deliberate switch or sandbox edit survives reinstall; no key is
    // invented even if a lamp enabled the switch before provisioning it.
    fs::write(lamp.on(env), "LAMP_SKILLS_STORE=on\n").unwrap();
    fs::write(lamp.on(drop_in), "[Service]\nPrivateNetwork=yes\n").unwrap();
    assert!(lamp.install().unwrap().changed.is_empty());
    assert_eq!(
        fs::read_to_string(lamp.on(env)).unwrap(),
        "LAMP_SKILLS_STORE=on\n"
    );
    assert_eq!(
        fs::read_to_string(lamp.on(drop_in)).unwrap(),
        "[Service]\nPrivateNetwork=yes\n"
    );
    assert!(!lamp.on(lamp_skills::package::STORE_KEY).exists());
    // Store provision is installer-only: the settings writer takes none of
    // these paths. The factory's key never enters a build manifest; a
    // build ships the Store's key only in the skills folder, where
    // lamp-skills reads it (#3692, `install::store_key_setting`).
    let carried = install::product_settings();
    assert!(
        carried.iter().all(|s| s.path != env
            && s.path != drop_in
            && s.path != lamp_skills::package::STORE_KEY)
    );
    assert_eq!(
        install::STORE_KEY_SETTING,
        format!(
            "{}/{}",
            lamp_update::product_skills::FOLDER,
            lamp_skills::package::BUILD_STORE_KEY
        )
    );
}

#[test]
fn a_missing_store_drop_in_is_repaired_and_failed_reload_is_retried() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let drop_in = "/etc/systemd/system/lamp-skills-manage@.service.d/store.conf";
    fs::remove_file(lamp.on(drop_in)).unwrap();
    lamp.host.failing_enables = 1;
    assert!(lamp.install().is_err());
    assert!(lamp.state().systemd_owed);
    assert!(lamp.on(drop_in).exists());
    lamp.install().unwrap();
    assert!(!lamp.state().systemd_owed);
    assert!(lamp.install().unwrap().changed.is_empty());
}

#[test]
fn install_places_every_exit_note_hook_and_keeps_other_drop_ins() {
    let mut lamp = Lamp::new(188, &[]);
    let other = "/etc/systemd/system/lamp-voice.service.d/30-local.conf";
    let prefix = "/etc/systemd/system/lamp-.service.d/exit-note.conf";
    for path in [other, prefix] {
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
        fs::write(lamp.on(path), "# this lamp's existing drop-in\n").unwrap();
    }
    lamp.install().unwrap();
    for name in install::EXIT_NOTE_UNITS {
        let path = format!("/etc/systemd/system/{name}.d/exit-note.conf");
        assert_eq!(
            fs::read_to_string(lamp.on(&path)).unwrap(),
            install::EXIT_NOTE
        );
        assert_eq!(mode_of(&lamp.on(&path)), 0o644, "{name}");
    }
    for path in [other, prefix] {
        assert_eq!(
            fs::read_to_string(lamp.on(path)).unwrap(),
            "# this lamp's existing drop-in\n"
        );
    }
    assert!(
        !lamp
            .on("/etc/systemd/system/ssh.service.d/exit-note.conf")
            .exists()
    );
    assert!(
        !lamp
            .on("/etc/systemd/system/lamp-records.timer.d/exit-note.conf")
            .exists()
    );
    // Installing the hooks neither runs them nor enables reporting consent.
    for path in [
        "/run/lampos-exits",
        "/etc/lampOS/setup/reports.json",
        "/var/lib/lampos-cloud/reports.json",
    ] {
        assert!(!lamp.on(path).exists(), "{path}");
    }
    let before = tree(&lamp.root);
    let calls = lamp.host.calls.len();
    assert!(lamp.install().unwrap().changed.is_empty());
    assert_eq!(tree(&lamp.root), before);
    assert_eq!(lamp.host.calls.len(), calls);

    // A missing reserved hook is repaired and reload debt is paid, with
    // every other drop-in kept. No service is restarted by installation.
    let missing = "/etc/systemd/system/lamp-skills.service.d/exit-note.conf";
    fs::remove_file(lamp.on(missing)).unwrap();
    let done = lamp.install().unwrap();
    assert_eq!(done.changed.len(), 2, "{:?}", done.changed);
    assert_eq!(done.changed[0], format!("wrote {missing}"));
    assert!(done.changed[1].starts_with("enabled "));
    assert_eq!(tree(&lamp.root), before);
    assert_eq!(
        lamp.host.calls[calls..]
            .iter()
            .map(|call| call.split(' ').next().unwrap())
            .collect::<Vec<_>>(),
        ["tmpfiles", "enable"]
    );
}

/// Every line install said that names a file turning a switch off.
fn switches_said(lamp: &Lamp) -> Vec<&str> {
    lamp.said
        .iter()
        .map(String::as_str)
        .filter(|line| line.ends_with("; the product sets it on"))
        .collect()
}

/// A lamp from its builder days with an old `LAMP_CLOUD=off` drop-in: the
/// install to stable keeps it, as it keeps every drop-in but its own, and
/// names it, at every run. A drop-in that sets other variables is not
/// named (one that sets the switch back on: the test after this one).
#[test]
fn a_kept_drop_in_that_turns_lamp_cloud_off_is_named_and_kept() {
    let mut lamp = Lamp::new(188, &[]);
    let old = "/etc/systemd/system/lamp-cloud.service.d/old.conf";
    let kept = [
        (
            old,
            "[Service]\nEnvironment=LAMP_CLOUD_OFFLINE_WILL=on \"LAMP_CLOUD=off\"\n",
        ),
        (
            "/etc/systemd/system/lamp-cloud.service.d/on.conf",
            "[Service]\nEnvironment=LAMP_CLOUD_OFFLINE_WILL=on LAMP_OFFICE=off\n",
        ),
        (
            "/etc/systemd/system/lamp-voice.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_OFFICE=on\n",
        ),
    ];
    for (path, text) in kept {
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
        fs::write(lamp.on(path), text).unwrap();
    }
    lamp.install().unwrap();
    let line = format!("kept drop-in {old} turns LAMP_CLOUD off (\"off\"); the product sets it on");
    assert_eq!(switches_said(&lamp), [line.as_str()], "{:?}", lamp.said);
    for (path, text) in kept {
        assert_eq!(fs::read_to_string(lamp.on(path)).unwrap(), text, "{path}");
    }
    lamp.said.clear();
    assert!(lamp.install().unwrap().changed.is_empty());
    assert_eq!(switches_said(&lamp), [line.as_str()], "said again");
    // Taken back on, it is said no more.
    fs::write(lamp.on(old), "[Service]\nEnvironment=LAMP_CLOUD=on\n").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);
}

/// Read as systemd merges them (the Reviewer-lead's P3s on #3728): an old
/// drop-in's `off` that a later drop-in, or an environment file, sets back
/// `on` leaves the switch on, and nothing is said. An environment file named
/// twice, with another between, is said once.
#[test]
fn a_switch_set_back_on_later_is_not_named_and_a_file_named_twice_is_said_once() {
    let mut lamp = Lamp::new(188, &[]);
    let folder = "/etc/systemd/system/lamp-cloud.service.d";
    fs::create_dir_all(lamp.on(folder)).unwrap();
    fs::write(
        lamp.on(&format!("{folder}/10-old.conf")),
        "[Service]\nEnvironment=LAMP_CLOUD=off\n",
    )
    .unwrap();
    fs::write(
        lamp.on(&format!("{folder}/20-fix.conf")),
        "[Service]\nEnvironment=LAMP_CLOUD=on\n",
    )
    .unwrap();
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);

    // Off in every drop-in, on in store.env, which wins over Environment=.
    fs::write(
        lamp.on(&format!("{folder}/20-fix.conf")),
        "[Service]\nEnvironment=LAMP_CLOUD=off\n",
    )
    .unwrap();
    let env = "/etc/lampOS/skills/store.env";
    fs::write(lamp.on(env), "LAMP_CLOUD=on\n").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);

    // store.env named again by a drop-in after another file: read last, said once.
    fs::write(lamp.on(env), "LAMP_CLOUD=off\n").unwrap();
    let other = "/etc/lampOS/cloud/other.env";
    fs::create_dir_all(lamp.on(other).parent().unwrap()).unwrap();
    fs::write(lamp.on(other), "LAMP_OFFICE=on\n").unwrap();
    fs::write(
        lamp.on(&format!("{folder}/30-files.conf")),
        format!("[Service]\nEnvironmentFile=-{other}\nEnvironmentFile=-{env}\n"),
    )
    .unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        switches_said(&lamp),
        [format!(
            "edited setting {env}, read by lamp-cloud.service, turns LAMP_CLOUD off (\"off\"); the product sets it on"
        )],
        "{:?}",
        lamp.said
    );
}

/// A clean install, and the next, name nothing: no product setting or
/// unit of the product's own turns a switch of it off.
#[test]
fn a_clean_install_names_no_switch_off() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);
    let mut setup = Lamp::new(188, &[]);
    setup.config.setup = true;
    setup.install().unwrap();
    assert!(switches_said(&setup).is_empty(), "{:?}", setup.said);
}

/// An edited setting that turns a switch off: `store.env`, kept as the
/// lamp's own Store choice, is lamp-cloud's `EnvironmentFile=`, which
/// systemd reads over the unit's `Environment=`. An environment file a kept
/// drop-in names is read too. The Store's own switch, off in the product, is
/// no switch of install's to guard.
#[test]
fn an_edited_setting_or_environment_file_that_turns_a_switch_off_is_named() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let env = "/etc/lampOS/skills/store.env";
    fs::write(lamp.on(env), "LAMP_SKILLS_STORE=on\n").unwrap();
    lamp.install().unwrap();
    assert!(switches_said(&lamp).is_empty(), "{:?}", lamp.said);
    fs::write(lamp.on(env), "LAMP_SKILLS_STORE=off\nLAMP_CLOUD=off\n").unwrap();
    let drop_in = "/etc/systemd/system/lamp-cloud.service.d/old.conf";
    let file = "/etc/lampOS/cloud/old.env";
    fs::create_dir_all(lamp.on(drop_in).parent().unwrap()).unwrap();
    fs::create_dir_all(lamp.on(file).parent().unwrap()).unwrap();
    fs::write(
        lamp.on(drop_in),
        format!(
            "[Service]\nEnvironmentFile=-{file}\nEnvironmentFile=-/etc/lampOS/cloud/none.env\n"
        ),
    )
    .unwrap();
    fs::write(lamp.on(file), "LAMP_SKILLS_PAGE=\"\"\nLAMP_OFFICE=off\n").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        switches_said(&lamp),
        [
            format!(
                "edited setting {env}, read by lamp-cloud.service, turns LAMP_CLOUD off (\"off\"); the product sets it on"
            ),
            format!(
                "environment file {file}, read by lamp-cloud.service, turns LAMP_SKILLS_PAGE off (\"\"); the product sets it on"
            ),
        ],
        "{:?}",
        lamp.said
    );
    assert_eq!(
        fs::read_to_string(lamp.on(env)).unwrap(),
        "LAMP_SKILLS_STORE=off\nLAMP_CLOUD=off\n"
    );
}

#[test]
fn a_failed_exit_note_write_keeps_reload_debt_for_the_next_install() {
    let mut lamp = Lamp::new(188, &[]);
    let blocked = "/etc/systemd/system/lamp-addons.service.d/exit-note.conf";
    fs::create_dir_all(lamp.on(blocked)).unwrap();
    let why = lamp.install().unwrap_err();
    assert!(why.contains(blocked), "{why}");
    assert!(why.starts_with("units not all written: "), "{why}");
    assert!(lamp.state().systemd_owed);
    assert!(
        lamp.on("/etc/systemd/system/lamp-actions.service.d/exit-note.conf")
            .is_file()
    );
    assert!(
        !lamp
            .host
            .calls
            .iter()
            .any(|call| call.starts_with("enable "))
    );
    fs::remove_dir(lamp.on(blocked)).unwrap();
    lamp.install().unwrap();
    assert!(!lamp.state().systemd_owed);
    for name in install::EXIT_NOTE_UNITS {
        assert_eq!(
            fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{name}.d/exit-note.conf")))
                .unwrap(),
            install::EXIT_NOTE
        );
    }
    let calls = lamp.host.calls.len();
    assert!(lamp.install().unwrap().changed.is_empty());
    assert_eq!(lamp.host.calls.len(), calls);
}

#[test]
fn a_file_that_does_not_match_installs_nothing_of_it() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.channel.files.insert(
        format!("{CHANNEL}lamp-body"),
        b"not what was signed\n".to_vec(),
    );
    let why = lamp.install().unwrap_err();
    assert!(
        why.contains("lamp-body does not match its SHA-256"),
        "{why}"
    );
    let body = lamp.on("/opt/lampOS/body");
    let left: Vec<_> = fs::read_dir(&body).unwrap().collect();
    assert!(left.is_empty(), "nothing of the body is left: {left:?}");
    assert!(!lamp.on("/etc/systemd/system/lamp-body.service").exists());
    // No part is linked to a build of a run that failed.
    assert!(fs::symlink_metadata(lamp.on("/opt/lampOS/voice/current")).is_err());
}

#[test]
fn a_build_without_a_part_is_refused_before_anything_is_written() {
    let mut lamp = Lamp::new(188, &["lamp-eyes"]);
    let why = lamp.install().unwrap_err();
    assert!(why.contains("build 188 carries no lamp-eyes"), "{why}");
    assert!(!lamp.on("/opt/lampOS").exists());
    assert!(!lamp.on("/etc/systemd").exists());
}

/// #2249 (the reviewer's P2): a build from before the body's factory tools
/// (19 Oct) still installs every part; the tools are said missing, once
/// each, and not put beside the body.
#[test]
fn a_build_without_the_bodys_factory_tools_still_installs() {
    let mut lamp = Lamp::new(188, &["lamp-body-measure", "lamp-body-mood"]);
    lamp.install().unwrap();
    assert!(lamp.on("/opt/lampOS/body/current/lamp-body").is_file());
    assert!(lamp.on("/opt/lampOS/voice/current/lamp-turn-end").is_file());
    for tool in ["lamp-body-measure", "lamp-body-mood"] {
        assert!(
            !lamp
                .on(&format!("/opt/lampOS/body/current/{tool}"))
                .exists()
        );
        let said: Vec<&String> = lamp
            .said
            .iter()
            .filter(|line| line.contains(tool))
            .collect();
        assert_eq!(said.len(), 1, "{tool}: {:?}", lamp.said);
        assert!(said[0].contains("build 188 carries no"), "{said:?}");
    }
}

#[test]
fn a_newer_build_moves_each_link_and_keeps_the_old_folders() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let old = fs::read_link(lamp.on("/opt/lampOS/voice/current")).unwrap();
    let mut newer = Lamp::new(189, &[]);
    newer.root = lamp.root.clone();
    newer.config.root = lamp.root.clone();
    newer.host.root = lamp.root.clone();
    // The image's key is the newer channel's: the lamp is given it again.
    newer.install().unwrap();
    let new = fs::read_link(newer.on("/opt/lampOS/voice/current")).unwrap();
    assert_ne!(old, new);
    assert!(
        lamp.on("/opt/lampOS/voice").join(&old).is_dir(),
        "the old build stays"
    );
}

/// Every component of the brain with a process of its own has a product
/// unit (docs/brain-build.md, "Components, processes and owners").
#[test]
fn every_part_of_the_brain_has_a_product_unit() {
    let page = include_str!("../../../docs/brain-build.md");
    let table = page
        .split("## Components, processes and owners")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .find(|block| block.starts_with("| Component"))
        .unwrap();
    let mut named = Vec::new();
    for row in table.lines().skip(2) {
        let process = row.split('|').nth(2).unwrap_or_default();
        // The first `lamp-<name>` in the process column; the bus is a
        // library, not a process.
        let Some(start) = process.find("`lamp-") else {
            continue;
        };
        let name: String = process[start + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if name != "lamp-bus" {
            named.push(name);
        }
    }
    assert!(named.len() >= 12, "the table names {named:?}");
    let units: Vec<&str> = PRODUCT.iter().map(|(name, _)| *name).collect();
    for name in &named {
        let unit = format!("{name}.service");
        assert!(units.contains(&unit.as_str()), "{name} has no product unit");
    }
}

/// Setup's marker (agreed with the third on #811, 2026-10-15 17:07; the
/// `marker` of setup's plan).
const SETUP_DONE: &str = "/var/lib/lampos-setup/done";

/// The lines of one section of a unit, without comments.
fn section<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let header = format!("[{name}]");
    let mut inside = false;
    let mut lines = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == header;
        } else if inside && !line.is_empty() && !line.starts_with('#') {
            lines.push(line);
        }
    }
    lines
}

/// Nothing of the lamp starts before setup has finished, on any boot
/// (#806). systemd checks a unit's condition only when its own job runs,
/// after the jobs it pulls in are queued, so the target's condition holds
/// back none of its parts (the reviewer on #815): each part in the target
/// carries the condition itself, in `[Unit]` where systemd reads it. The
/// units outside the target listen to nothing and run before setup, but
/// setup itself, which runs only while the marker is missing, and the path
/// unit that starts the lamp once it is there (#811).
#[test]
fn every_part_of_the_lamp_waits_for_setups_marker_itself() {
    let condition = format!("ConditionPathExists={SETUP_DONE}");
    let mut gated = Vec::new();
    for (name, text) in PRODUCT {
        let unit = section(text, "Unit");
        let part_of = unit.contains(&"PartOf=lampos.target");
        let wanted = section(text, "Install").contains(&"WantedBy=lampos.target");
        assert_eq!(part_of, wanted, "{name}: in the target one way only");
        if part_of || *name == "lampos.target" {
            assert!(
                unit.contains(&condition.as_str()),
                "{name} starts before setup"
            );
            gated.push(*name);
        } else if *name == "lamp-setup.service" {
            // Setup is the one that runs while the marker is missing.
            assert!(unit.contains(&format!("ConditionPathExists=!{SETUP_DONE}").as_str()));
        } else if *name == "lamp-waking.service" {
            // The waking face is before the lamp, on a set-up lamp only:
            // before the marker the ring is the setup light's (#1681).
            assert!(unit.contains(&condition.as_str()), "{name}");
            assert!(section(text, "Install").contains(&"WantedBy=sysinit.target"));
        } else if *name == "lamp-setup-recover.service" {
            // Recovery is for a set-up lamp only (#3020): before the
            // marker, setup itself serves.
            assert!(unit.contains(&condition.as_str()), "{name}");
            assert!(section(text, "Install").contains(&"WantedBy=multi-user.target"));
        } else if *name == "lampos-setup-done.path" {
            // And this starts the lamp once it is there.
            let path = section(text, "Path");
            assert!(path.contains(&format!("PathExists={SETUP_DONE}").as_str()));
            assert!(path.contains(&"Unit=lampos.target"));
        } else {
            assert!(
                !text.contains("lampos-setup"),
                "{name} is outside the target and names setup"
            );
        }
    }
    assert_eq!(gated.len(), 24, "{gated:?}");
    for outside in ["lamp-update.service", "lamp-records.timer"] {
        assert!(!gated.contains(&outside), "{outside}");
    }
    let page = include_str!("../../../docs/install.md");
    assert!(page.contains(&format!("`{condition}`")));
}

/// #2848: every start of the voice starts the waking face again, so a
/// restart from the app, an update round or a crash opens the ring like an
/// eye before the voice takes it, as power-on does. The face is wanted and
/// ordered before the voice, and stays outside the target and unrestarted,
/// so a stop of the voice or a restart of the target leaves it alone.
#[test]
fn every_start_of_the_voice_starts_the_waking_face() {
    let unit = |wanted: &str| {
        PRODUCT
            .iter()
            .find(|(name, _)| *name == wanted)
            .map(|(_, text)| *text)
            .unwrap()
    };
    let voice = section(unit("lamp-voice.service"), "Unit");
    assert!(voice.contains(&"Wants=lamp-waking.service"), "{voice:?}");
    assert!(voice.contains(&"After=lamp-waking.service"), "{voice:?}");
    let waking = unit("lamp-waking.service");
    assert!(
        !section(waking, "Unit")
            .iter()
            .any(|line| line.starts_with("PartOf="))
    );
    assert!(section(waking, "Service").contains(&"Restart=no"));
    assert!(section(waking, "Service").contains(&"Environment=LAMP_WAKING_FACE=on"));
}

/// #3710: each product unit that runs as root has the sandbox its work
/// needs: a read-only system but for the exact paths it writes, no home,
/// its own /tmp and no privileges gained. One row a unit, each line once.
#[test]
fn root_units_write_only_what_their_work_needs() {
    // (unit, what it writes, its own /tmp)
    let rows: [(&str, &[&str], bool); 2] = [
        // The release socket, in its runtime directory, and the ring's
        // device: nothing else (crates/setup/src/wake.rs). No PrivateTmp:
        // it would order this early unit after the /tmp mounts and
        // systemd-tmpfiles-setup, delaying first light (Reviewer-lead's P3
        // on #4036), and it uses no /tmp.
        ("lamp-waking.service", &[], false),
        // Its subscriber's socket under the bus; the reset is detached
        // (below), so the wipe is not this unit's to write.
        ("lamp-setup-reset.service", &["/run/lampos-bus"], true),
    ];
    // The reset's wipe, stop and reboot run in a unit of their own
    // (`start_reset`), so watch-reset's sandbox never has to hold them.
    let plan: serde_json::Value = serde_json::from_str(install::RESET_PLAN).unwrap();
    assert_eq!(plan["detach"][0], "/usr/bin/systemd-run");
    for (unit, writes, private_tmp) in rows {
        let text = PRODUCT
            .iter()
            .find_map(|(name, text)| (*name == unit).then_some(*text))
            .unwrap();
        let service = section(text, "Service");
        assert!(service.contains(&"User=root"), "{unit}");
        for line in [
            "ProtectSystem=strict",
            "ProtectHome=yes",
            "NoNewPrivileges=yes",
        ] {
            assert_eq!(
                service.iter().filter(|each| **each == line).count(),
                1,
                "{unit}: {line}"
            );
        }
        let tmp: Vec<&&str> = service
            .iter()
            .filter(|line| line.starts_with("PrivateTmp="))
            .collect();
        assert_eq!(
            tmp,
            if private_tmp {
                vec![&"PrivateTmp=yes"]
            } else {
                vec![]
            },
            "{unit}"
        );
        let written: Vec<&str> = service
            .iter()
            .filter_map(|line| line.strip_prefix("ReadWritePaths="))
            .flat_map(str::split_whitespace)
            .collect();
        assert_eq!(written, writes, "{unit}");
    }
}

/// #811: no battery clock. Every product part that acts on the time of day
/// trusts the wall clock only once the kernel says a time client set it:
/// the schedule, Expression's night and the voice's telling of the time.
#[test]
fn time_of_day_parts_wait_for_the_kernel_to_say_the_clock_is_set() {
    let unit = |name: &str| {
        install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    };
    let exec = |name: &str| {
        section(unit(name), "Service")
            .into_iter()
            .find(|line| line.starts_with("ExecStart="))
            .unwrap()
            .to_owned()
    };
    let schedule = exec("lamp-schedule.service");
    assert!(schedule.contains(" --kernel-clock") && !schedule.contains("--trust-clock"));
    assert!(exec("lamp-expression.service").contains(" --kernel-clock"));
    let voice = section(unit("lamp-voice.service"), "Service");
    assert!(voice.contains(&"Environment=LAMP_KERNEL_CLOCK=on"));
}

/// The lead's decision 1 (23 Oct): a product lamp keeps nothing about a
/// person while nobody is known, so an unknown speaker's "my favourite
/// colour is blue" is not kept to the room. The switch acts on the
/// `remember` the memory socket gives the voice. And what the room did keep
/// is never told as the asker's (`LAMP_MEMORY_ROOM_NOT_ASKER`, #3593: on
/// lamp-4ace on 26 Oct, "Black, no sugar, that's how you've had it" from
/// the room's talk, in 3 of 3 rounds without it).
#[test]
fn the_product_keeps_nothing_personal_while_nobody_is_known() {
    let voice = install::PRODUCT
        .iter()
        .find(|(unit, _)| *unit == "lamp-voice.service")
        .unwrap()
        .1;
    let service = section(voice, "Service");
    for line in [
        "Environment=LAMP_MEMORY_SOCKET=/run/lampos-memory/memory.sock",
        "Environment=LAMP_MEMORY_PERSONAL_NEEDS_FACE=on",
        "Environment=LAMP_MEMORY_ROOM_NOT_ASKER=on",
        "Environment=LAMP_MEMORY_FORGET_ASKS=on",
    ] {
        assert!(service.contains(&line), "the voice lacks {line}");
    }
}

/// A thing the person already keeps, said again, is that kept thing, not a
/// second one (`LAMP_MEMORY_REMEMBER_ONCE`, #2176): on lamp-06c1 on 28 Oct
/// the product unit, without it, kept "the meeting room is called orange"
/// twice (Friday's run on #3504).
#[test]
fn the_product_keeps_a_repeated_thing_once() {
    let voice = install::PRODUCT
        .iter()
        .find(|(unit, _)| *unit == "lamp-voice.service")
        .unwrap()
        .1;
    let service = section(voice, "Service");
    for line in [
        "Environment=LAMP_MEMORY_DAYS=on",
        "Environment=LAMP_MEMORY_REMEMBER_ONCE=on",
    ] {
        assert!(service.contains(&line), "the voice lacks {line}");
    }
}

/// Within a conversation going on, a known person's line no face was seen
/// speaking stays theirs to be told from (`LAMP_CONVERSATION_CARRIES`, #4339),
/// beside the scope by sight it acts with: on lamp-3e83 on 29 Oct one such
/// line dropped to the room's and the lamp asked its question again.
#[test]
fn the_product_carries_a_conversation_through_an_unseen_line() {
    let voice = install::PRODUCT
        .iter()
        .find(|(unit, _)| *unit == "lamp-voice.service")
        .unwrap()
        .1;
    let service = section(voice, "Service");
    for line in [
        "Environment=LAMP_PERSON_SCOPE_SEEN=on",
        "Environment=LAMP_CONVERSATION_CARRIES=on",
    ] {
        assert!(service.contains(&line), "the voice lacks {line}");
    }
}

/// A long conversation keeps its rules and score past the thread's view
/// (`LAMP_CONVERSATION_SUMMARY`, #1693's long-conversation plan, step 3): on
/// lamp-06c1 on 29 Oct, 11 exchanges after a quiz's rules the lamp said
/// there was no quiz.
#[test]
fn the_product_summarises_a_long_conversation() {
    let voice = install::PRODUCT
        .iter()
        .find(|(unit, _)| *unit == "lamp-voice.service")
        .unwrap()
        .1;
    let service = section(voice, "Service");
    assert!(
        service.contains(&"Environment=LAMP_CONVERSATION_SUMMARY=on"),
        "the voice lacks LAMP_CONVERSATION_SUMMARY=on"
    );
}

/// #2860: the product sees who speaks, the eyes reading each face's speaking
/// and the voice reading it, so a full update gives one lamp what the
/// real-face run had.
#[test]
fn the_product_sees_who_speaks() {
    let unit = |name: &str| {
        install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    };
    for (name, line) in [
        ("lamp-eyes.service", "Environment=LAMP_EYES_SPEAKING=on"),
        (
            "lamp-voice.service",
            "Environment=LAMP_SECOND_SPEAKING_FACE=on",
        ),
        // The voice reads the eyes' switch too: a room's forget needs the
        // face seen saying it, and without it every one is NOT_THEIRS
        // (#3037, #3038).
        ("lamp-voice.service", "Environment=LAMP_EYES_SPEAKING=on"),
    ] {
        assert!(
            section(unit(name), "Service").contains(&line),
            "{name} lacks {line}"
        );
    }
}

/// #1176 (Nine and the reviewer): Expression's `--head-touch` only beside
/// the voice's own head touch and its sleep mute (`LAMP_HEAD_TOUCH=on` and
/// `--controls-mute`). Without the mute a hand held 3 s puts the face to
/// sleep with the microphones open. Pins that give the head pads to the old
/// engine (`legacy_head`) are for a voice and an Expression that read
/// neither.
#[test]
fn head_touch_comes_whole_and_never_beside_the_old_engines_head() {
    let service = |name: &str| {
        let text = install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1;
        format!(" {} ", section(text, "Service").join(" "))
    };
    assert!(
        service("lamp-controls.service")
            .contains(" --pins /etc/lampOS/controls/controls-pins.json "),
        "install preflight must check the product Controls reader's active pins"
    );
    let voice = service("lamp-voice.service");
    let voice_head = voice.contains(" Environment=LAMP_HEAD_TOUCH=on ");
    let sleep_mute = voice.contains(" --controls-mute ");
    let face_head = service("lamp-expression.service").contains(" --head-touch ");
    assert!(
        !face_head || (voice_head && sleep_mute),
        "Expression's --head-touch without the voice's head touch and sleep mute"
    );
    let pins = SETTINGS
        .iter()
        .find(|(path, _)| *path == "/etc/lampOS/controls/controls-pins.json")
        .unwrap()
        .1;
    let pins: serde_json::Value = serde_json::from_str(pins).unwrap();
    if pins["legacy_head"] == true {
        assert!(
            !voice_head && !face_head,
            "the old engine's head beside the voice's or Expression's head touch"
        );
    }
}

/// #811 / #806 (the first builder, 12:36): a home lamp's voice uses V1's
/// route with the lamp's own device key, which the customer never types.
/// Install writes the product `provider.json` and takes the key from V1's
/// config, both private as the voice requires; it never says the key, keeps
/// a key already there, and says when there is none.
#[test]
fn the_voice_gets_its_service_and_the_lamps_device_key_from_v1s_config() {
    use std::os::unix::fs::PermissionsExt;
    const KEY: &str = "dev-key-0123456789abcdef";
    let private =
        |lamp: &Lamp, path: &str| fs::metadata(lamp.on(path)).unwrap().permissions().mode() & 0o777;

    let mut lamp = Lamp::new(188, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(
        lamp.on(install::V1_CONFIG),
        format!(r#"{{"llm_api_key": "{KEY}", "device_id": "lamp-06c1"}}"#),
    )
    .unwrap();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        KEY
    );
    assert_eq!(private(&lamp, install::CREDENTIAL_PATH), 0o600);
    assert_eq!(
        fs::read_to_string(lamp.on(install::PROVIDER_PATH)).unwrap(),
        install::PROVIDER
    );
    assert_eq!(private(&lamp, install::PROVIDER_PATH), 0o600);
    let provider: serde_json::Value = serde_json::from_str(install::PROVIDER).unwrap();
    assert_eq!(provider["credential_file"], install::CREDENTIAL_PATH);
    assert_eq!(
        provider["resumption"], "off",
        "the proxy does not forward it"
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("V1's key was taken"))
    );
    // The Policy's judge key is the same device key, as private (#811, 2).
    assert_eq!(
        fs::read_to_string(lamp.on(install::JEV_KEY_PATH)).unwrap(),
        KEY
    );
    assert_eq!(private(&lamp, install::JEV_KEY_PATH), 0o600);
    let policy = section(
        install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-policy.service")
            .unwrap()
            .1,
        "Service",
    );
    let exec = policy
        .iter()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(
        exec.contains("--endpoint https://campaign-api.autonomous.ai/api/v1/ai/v1/jev/decisions")
    );
    assert!(policy.contains(&"LoadCredential=jev.key:/etc/lampOS/policy/jev.key"));
    assert!(
        lamp.said.iter().all(|line| !line.contains(KEY)),
        "{:?}",
        lamp.said
    );
    let before = tree(&lamp.root);
    assert_eq!(lamp.install().unwrap().changed, Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);

    // V1 rotates the key (the reviewer, thirteen): install's own copy
    // follows it, still private.
    fs::write(
        lamp.on(install::V1_CONFIG),
        r#"{"llm_api_key": "rotated-key-42"}"#,
    )
    .unwrap();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        "rotated-key-42"
    );
    assert_eq!(
        fs::read_to_string(lamp.on(install::JEV_KEY_PATH)).unwrap(),
        "rotated-key-42"
    );
    assert_eq!(private(&lamp, install::CREDENTIAL_PATH), 0o600);

    // A key written on the lamp by someone else (the factory) is the
    // lamp's: kept, even when V1's differs, and never said.
    fs::write(lamp.on(install::CREDENTIAL_PATH), "factory-key").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        "factory-key"
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {}", install::CREDENTIAL_PATH)))
    );
    assert!(lamp.said.iter().all(|line| !line.contains("factory-key")));

    // No V1 config: nothing written, and said.
    let mut bare = Lamp::new(188, &[]);
    bare.install().unwrap();
    assert!(!bare.on(install::CREDENTIAL_PATH).exists());
    assert!(!bare.on(install::JEV_KEY_PATH).exists());
    assert!(bare.on(install::PROVIDER_PATH).exists());
    assert!(bare.said.iter().any(|line| line.starts_with("no key")));
}

/// #1712 (the first builder at 21:08): the second route is the lamps' voice,
/// so a fresh install places its services, private, on the relay with the
/// voice's one key; a lamp's own file (lamp-4ace's) is kept, and said.
#[test]
fn the_second_route_gets_its_services_on_the_voices_key_and_a_lamps_own_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::SECOND_PATH)).unwrap(),
        install::SECOND
    );
    assert_eq!(
        fs::metadata(lamp.on(install::SECOND_PATH))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let second: serde_json::Value = serde_json::from_str(install::SECOND).unwrap();
    assert_eq!(second["llm_credential_file"], install::CREDENTIAL_PATH);
    // A lamp's own is the lamp's: kept on the next install.
    let own = r#"{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"/etc/lampOS/voice/second-llm.key"}"#;
    fs::write(lamp.on(install::SECOND_PATH), own).unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::SECOND_PATH)).unwrap(),
        own
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {}", install::SECOND_PATH)))
    );
}

/// The owner's decision on #806 (16:13): the voice's key is the user's, and
/// a reset wipes it with the person's data. V1's config on a lamp that ran
/// V1 may still hold the previous owner's key, so install, run again after
/// the reset (as every updater upgrade runs it), never puts it back; and a
/// key setup then takes from the new user is theirs, kept.
#[test]
fn a_key_a_reset_removed_is_never_put_back_from_v1s_config() {
    const OLD: &str = "previous-owners-key";
    let mut lamp = Lamp::new(188, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(
        lamp.on(install::V1_CONFIG),
        format!(r#"{{"llm_api_key": "{OLD}"}}"#),
    )
    .unwrap();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        OLD
    );
    // The 5 s reset wipes both keys with the person's data.
    fs::remove_file(lamp.on(install::CREDENTIAL_PATH)).unwrap();
    fs::remove_file(lamp.on(install::JEV_KEY_PATH)).unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert!(!lamp.on(install::CREDENTIAL_PATH).exists());
    assert!(!lamp.on(install::JEV_KEY_PATH).exists());
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("was removed since install wrote it")),
        "{:?}",
        lamp.said
    );
    assert!(lamp.said.iter().all(|line| !line.contains(OLD)));
    // Setup takes the new user's key: it is theirs, kept by every install.
    fs::write(lamp.on(install::CREDENTIAL_PATH), "new-users-key").unwrap();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        "new-users-key"
    );
}

/// #1094's review: install reads its records with `Records::state`. A
/// corrupt state file is set aside and said, never silently written over;
/// and since what install wrote before is then not known, a key a reset
/// removed is not taken for one never written: V1's is not copied back.
#[test]
fn corrupt_records_are_set_aside_and_never_bring_back_a_key_a_reset_removed() {
    const OLD: &str = "previous-owners-key";
    let mut lamp = Lamp::new(192, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(
        lamp.on(install::V1_CONFIG),
        format!(r#"{{"llm_api_key": "{OLD}"}}"#),
    )
    .unwrap();
    lamp.install().unwrap();
    // A reset wipes both keys, and the records are damaged.
    fs::remove_file(lamp.on(install::CREDENTIAL_PATH)).unwrap();
    fs::remove_file(lamp.on(install::JEV_KEY_PATH)).unwrap();
    let state = lamp.on("/var/lib/lampos-update/state.json");
    fs::write(&state, b"{ not json").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert!(!lamp.on(install::CREDENTIAL_PATH).exists());
    assert!(!lamp.on(install::JEV_KEY_PATH).exists());
    let aside: Vec<_> = fs::read_dir(state.parent().unwrap())
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.starts_with("state.json.corrupt-").then_some(name)
        })
        .collect();
    assert_eq!(aside.len(), 1, "the damaged file is kept beside");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("could not be read") && line.contains("begun again")),
        "{:?}",
        lamp.said
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("the records were set aside: V1's key is not copied")),
        "{:?}",
        lamp.said
    );
    assert!(lamp.said.iter().all(|line| !line.contains(OLD)));
    // What the set-aside means lasts (Third on #1263): the next install
    // does not take the missing keys for ones never written either.
    lamp.install().unwrap();
    assert!(!lamp.on(install::CREDENTIAL_PATH).exists());
    assert!(!lamp.on(install::JEV_KEY_PATH).exists());
    // The user's key, written by setup, is then kept as the lamp's own.
    fs::write(lamp.on(install::CREDENTIAL_PATH), "new-users-key").unwrap();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        "new-users-key"
    );
    assert!(!lamp.on(install::JEV_KEY_PATH).exists());
    // A blank lamp with no records at all still gets V1's key, as before.
    let mut blank = Lamp::new(193, &[]);
    fs::create_dir_all(blank.on("/root/config")).unwrap();
    fs::write(
        blank.on(install::V1_CONFIG),
        format!(r#"{{"llm_api_key": "{OLD}"}}"#),
    )
    .unwrap();
    blank.install().unwrap();
    assert_eq!(
        fs::read_to_string(blank.on(install::CREDENTIAL_PATH)).unwrap(),
        OLD
    );
}

/// An unreadable records file stops install: it is never written over.
#[test]
fn unreadable_records_stop_install_rather_than_being_written_over() {
    let mut lamp = Lamp::new(194, &[]);
    lamp.install().unwrap();
    let state = lamp.on("/var/lib/lampos-update/state.json");
    let kept = fs::read(&state).unwrap();
    fs::remove_file(&state).unwrap();
    // A folder where the file was: reading it fails, and not as missing.
    fs::create_dir(&state).unwrap();
    let refused = lamp.install().unwrap_err();
    assert!(refused.contains("the state cannot be read"), "{refused}");
    assert!(state.is_dir());
    fs::remove_dir(&state).unwrap();
    fs::write(&state, kept).unwrap();
    lamp.install().unwrap();
}

/// The repository's skill folders, `lampOS/skills/`.
fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

/// Every file of a skill folder but its tests, as paths in it.
fn skill_files(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, under: &str, files: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{under}{name}");
            if entry.path().is_dir() {
                if path != "tests" {
                    walk(&entry.path(), &format!("{path}/"), files);
                }
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(folder, "", &mut files);
    files.sort();
    files
}

/// The owner, 19 Oct: a lamp runs every behaviour that has a skill as the
/// skill. So every folder in `lampOS/skills/` is a product skill or says why
/// it isn't, and a new one is decided, not left out unseen.
#[test]
fn every_skill_folder_is_in_the_product_or_says_why_not() {
    let mut folders: Vec<String> = fs::read_dir(skills_root())
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    folders.sort();
    let mut decided: Vec<String> = install::PRODUCT_SKILLS
        .iter()
        .chain(install::NOT_PRODUCT_SKILLS.iter().map(|(name, _)| name))
        .map(|name| (*name).to_owned())
        .collect();
    decided.sort();
    assert_eq!(folders, decided);
    for (name, why) in install::NOT_PRODUCT_SKILLS {
        assert!(!why.is_empty(), "{name}");
    }
}

/// The owner: core skills are always installed, installable ones as apps.
/// Every folder of `lampOS/skills` whose `kind` is core is a product skill
/// (installed off if it is in `PRODUCT_SKILLS_OFF`), and every product
/// skill is core. Laugh glance is parked (#2524, #1669): out of both.
#[test]
fn the_product_skills_are_exactly_the_core_skills() {
    const PARKED: &[&str] = &["laugh-glance"];
    let mut core: Vec<String> = fs::read_dir(skills_root())
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().join("skill.toml").is_file())
        .map(|entry| lamp_skills::load(&entry.path()).unwrap())
        .filter(|skill| skill.kind == lamp_skills::SkillKind::Core)
        .map(|skill| skill.name)
        .filter(|name| !PARKED.contains(&name.as_str()))
        .collect();
    core.sort();
    let mut product: Vec<String> = install::PRODUCT_SKILLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    product.sort();
    assert_eq!(core, product);
    for (name, why) in install::PRODUCT_SKILLS_OFF {
        assert!(install::PRODUCT_SKILLS.contains(name), "{name}");
        assert!(!why.is_empty(), "{name}");
    }
}

/// #811, from lamp-06c1, and every skill since (the owner, 19 Oct): a home
/// lamp gets `lamp-skills` and every product skill whole but its tests,
/// byte for byte, and no other skill. The voice speaks them through its say
/// service; its own hello is off, so the lamp greets once, and its reminders
/// and task news wait for their skills' asks, so each is told once. Installed
/// skills still keep their own enabled default; installation does not run one.
#[test]
fn a_blank_lamp_runs_every_product_skill_as_the_skill() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(lamp.on("/opt/lampOS/skills/current/lamp-skills").exists());
    let mut installed = Vec::new();
    for skill in install::PRODUCT_SKILLS {
        let folder = skills_root().join(skill);
        let files = skill_files(&folder);
        assert!(files.contains(&"skill.toml".to_owned()), "{skill}");
        assert!(files.contains(&"SKILL.md".to_owned()), "{skill}");
        for file in files {
            let path = format!("/var/lib/lampOS/skills/{skill}/{file}");
            let mut ours = fs::read(folder.join(&file)).unwrap();
            // Installed disabled: the off line above its own file.
            if file == "skill.toml"
                && install::PRODUCT_SKILLS_OFF
                    .iter()
                    .any(|(off, _)| off == skill)
            {
                ours.splice(0..0, install::OFF_LINE.bytes());
            }
            assert_eq!(fs::read(lamp.on(&path)).unwrap(), ours, "{path}");
            installed.push(path);
        }
    }
    // Nothing else in the skills folder: no tests, no skill left out.
    let mut placed: Vec<String> = SETTINGS
        .iter()
        .map(|(path, _)| (*path).to_owned())
        .filter(|path| path.starts_with("/var/lib/lampOS/skills/"))
        .collect();
    placed.sort();
    installed.sort();
    assert_eq!(placed, installed);
    for (skill, _) in install::NOT_PRODUCT_SKILLS {
        assert!(
            !lamp.on(&format!("/var/lib/lampOS/skills/{skill}")).exists(),
            "{skill}"
        );
    }
    assert!(
        lamp.host
            .calls
            .iter()
            .any(|call| call.contains("lamp-skills.service"))
    );
    // A pet on the head is said in the product, and Head pat and its wiggle
    // answer it (#4133's design owner's yes, 29 Oct).
    let controls =
        fs::read_to_string(lamp.on("/etc/systemd/system/lamp-controls.service")).unwrap();
    assert!(section(&controls, "Service").contains(&"Environment=LAMP_HEAD_PET=on"));
    for skill in ["head-pat", "head-pat-wiggle"] {
        assert!(install::PRODUCT_SKILLS.contains(&skill), "{skill}");
        assert!(
            lamp.on(&format!("/var/lib/lampOS/skills/{skill}/skill.toml"))
                .is_file(),
            "{skill}"
        );
    }
    let voice = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-voice.service")).unwrap();
    let service = section(&voice, "Service");
    assert!(service.contains(&"Environment=LAMP_SAY=on"));
    assert!(service.contains(&format!("Environment=LAMP_SAY_GROUP={}", install::USER).as_str()));
    assert!(
        !service
            .iter()
            .any(|line| line.starts_with("Environment=LAMP_SPEAK_FIRST="))
    );
    // A due reminder and the news of a task waits up to 10 s for its skill's
    // ask, then the voice says it itself: never both, never neither. Each
    // set once, so no later line puts the voice's own timing back.
    for switch in ["LAMP_SCHEDULE_SPEAK", "LAMP_TASK_NEWS"] {
        let set: Vec<&str> = service
            .iter()
            .filter_map(|line| line.strip_prefix(&format!("Environment={switch}=")))
            .collect();
        assert_eq!(set, ["skill"], "{switch}");
    }
    let skills = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-skills.service")).unwrap();
    assert!(skills.contains("--say-socket /run/lampos-say/say.sock"));
    assert!(skills.contains("--dir /var/lib/lampOS/skills"));
    // Dance's tool reaches the model from lamp-skills' own folder, which the
    // voice reads at each session (#1656), not the actions part's.
    assert!(skills.contains("--tools-dir /run/lampos-skills"));
    // The eyes' known file for a watch (#1685), as an `Environment=` line:
    // an older lamp-skills would end at its start on `--known-file` (#2157).
    assert!(skills.contains("\nEnvironment=LAMP_SKILLS_KNOWN_FILE=/run/lampos-eyes/known\n"));
    let start = skills
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(!start.contains("--known-file"), "{start}");
    assert!(service.contains(&"Environment=LAMP_ACTIONS_DIR=/run/lampos-actions"));
}

/// The reviewer on #2585: a core skill's file changed by hand is written
/// back by the next install, as a round puts it back, and said so.
#[test]
fn an_install_puts_a_core_skills_changed_file_back_and_says_so() {
    let mut lamp = Lamp::new(189, &[]);
    lamp.install().unwrap();
    let path = "/var/lib/lampOS/skills/dance/skill.toml";
    let ours = fs::read(skills_root().join("dance/skill.toml")).unwrap();
    fs::write(lamp.on(path), "enabled = false\n").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(fs::read(lamp.on(path)).unwrap(), ours);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("put back {path}: changed on this lamp"))),
        "{:?}",
        lamp.said
    );
}

/// The first builder on #2585 (22:24): a disabled core skill turned on
/// stays on through an install. Bedtime and Morning ships off; turned on by
/// hand, the next install keeps it on and records it in lamp-skills' state
/// folder, so an install with its file gone writes it turned on again.
#[test]
fn a_disabled_core_skill_turned_on_stays_on_through_an_install() {
    let mut lamp = Lamp::new(196, &[]);
    lamp.install().unwrap();
    let state_dir = lamp.on("/var/lib/lampos-skills-state");
    fs::create_dir_all(&state_dir).unwrap();
    let folder = lamp.on("/var/lib/lampOS/skills/bedtime-morning");
    let toml = folder.join("skill.toml");
    let shipped = fs::read_to_string(&toml).unwrap();
    assert!(!lamp_skills::load(&folder).unwrap().enabled);
    fs::write(&toml, shipped.replace("enabled = false", "enabled = true")).unwrap();

    lamp.install().unwrap();
    assert!(lamp_skills::load(&folder).unwrap().enabled);
    assert_eq!(
        fs::read_to_string(state_dir.join("on")).unwrap(),
        "bedtime-morning\n"
    );
    // The choice is the record's, not only the file's.
    fs::remove_file(&toml).unwrap();
    lamp.install().unwrap();
    assert!(lamp_skills::load(&folder).unwrap().enabled);
}

#[test]
fn the_installed_bedtime_skill_is_disabled_and_absent_from_live_catalogs() {
    let mut lamp = Lamp::new(195, &[]);
    lamp.install().unwrap();
    let root = lamp.on("/var/lib/lampOS/skills");
    let installed: Vec<_> = install::PRODUCT_SKILLS
        .iter()
        .map(|name| lamp_skills::load(&root.join(name)).unwrap())
        .collect();
    // Not feeling well came with the product after its lamp check (#1681);
    // the seven of PRODUCT_SKILLS_OFF come installed disabled; the startle
    // came with its move (#2822); the four wellbeing nudges since 25 Oct;
    // Head pat and its wiggle since #4133's design owner's yes (29 Oct).
    assert_eq!(installed.len(), 27);
    let bedtime = installed
        .iter()
        .find(|skill| skill.name == "bedtime-morning")
        .unwrap();
    assert!(!bedtime.enabled);
    assert!(bedtime.tool.is_none());
    assert_eq!(bedtime.kind, lamp_skills::SkillKind::Core);
    assert_eq!(bedtime.version.as_deref(), Some("1.0.0"));
    assert_eq!(
        bedtime.permissions,
        lamp_skills::Permissions {
            topics: vec![
                "humans/faces/tracked".into(),
                "voice/state".into(),
                "voice/heard".into()
            ],
            speak: true,
            light: true,
            output_limits: true,
            ..Default::default()
        }
    );
    assert!(root.join("bedtime-morning/SKILL.md").is_file());
    // Inventory includes the disabled core and its grants; only live skills
    // contribute functions. The nineteen live skills (with music sway and
    // watch you leave since #2834, and Head pat and its wiggle, which declare
    // none) offer Dance,
    // prepare_to_unplug, hidden at run time unless LAMP_UNPLUG=on, and the
    // wellbeing nudges' one wellbeing_nudges (since 25 Oct).
    let inventory = lamp_skills::load_each(&root);
    assert_eq!(inventory.len(), 27);
    assert!(inventory.iter().all(|(_, skill)| skill.is_ok()));
    let listed = lamp_skills::package::list(&root).unwrap();
    let row = listed
        .lines()
        .find(|line| line.split_whitespace().next() == Some("bedtime-morning"))
        .unwrap();
    assert_eq!(
        row.split_whitespace().take(4).collect::<Vec<_>>(),
        ["bedtime-morning", "core", "1.0.0", "disabled;"]
    );
    assert!(
        listed
            .lines()
            .last()
            .unwrap()
            .starts_with(
                "The skills here give the model 3 functions (dance, prepare_to_unplug, wellbeing_nudges)."
            ),
        "{listed}"
    );
    let (live, errors) = lamp_skills::load_all(&root);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(live.len(), 19);
    assert!(live.iter().all(|skill| skill.name != bedtime.name));
    // Each core skill installed off is disabled on the lamp and adds no
    // function, its tool or not: the 3 above are all a session is told.
    for (name, _) in install::PRODUCT_SKILLS_OFF {
        let skill = installed.iter().find(|skill| skill.name == *name).unwrap();
        assert!(!skill.enabled, "{name}");
        assert_eq!(skill.kind, lamp_skills::SkillKind::Core, "{name}");
        assert!(live.iter().all(|live| live.name != *name), "{name}");
        assert_eq!(
            lamp_skills::tools_file(std::slice::from_ref(skill)),
            serde_json::json!({"tools":[]}),
            "{name}"
        );
    }
    assert_eq!(
        lamp_skills::tools_file(&installed),
        lamp_skills::tools_file(&live)
    );
    // Even a caller that passes the disabled file directly cannot subscribe
    // its triggers or advertise it beside the product's live tools.
    assert!(
        lamp_skills::Engine::new(vec![bedtime.clone()], true)
            .topics()
            .is_empty()
    );
    assert_eq!(
        lamp_skills::tools_file(std::slice::from_ref(bedtime)),
        serde_json::json!({"tools":[]})
    );
}

#[test]
fn product_routine_state_is_private_persistent_and_keeps_new_switches_off() {
    let mut lamp = Lamp::new(196, &[]);
    lamp.install().unwrap();
    for (unit, directory, variable, flag, path) in [
        (
            "lamp-skills.service",
            "lampos-skills-state",
            "LAMP_SKILLS_DAILY_FILE",
            "--daily-state",
            "/var/lib/lampos-skills-state/daily.json",
        ),
        (
            "lamp-actions.service",
            "lampos-output-caps",
            "LAMP_OUTPUT_CAPS_DIR",
            "--caps-dir",
            "/var/lib/lampos-output-caps",
        ),
    ] {
        let installed =
            fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{unit}"))).unwrap();
        let service = section(&installed, "Service");
        for required in [
            format!("User={}", install::USER),
            format!("Group={}", install::USER),
            format!("StateDirectory={directory}"),
            "StateDirectoryMode=0700".into(),
            "ProtectSystem=strict".into(),
            format!("Environment={variable}={path}"),
        ] {
            assert_eq!(
                service.iter().filter(|line| **line == required).count(),
                1,
                "{unit}: {required}"
            );
        }
        // The path comes by environment, never the new flag (#2157).
        let command = service
            .iter()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .unwrap();
        assert!(
            !command.split_whitespace().any(|word| word == flag),
            "{unit}"
        );
        assert!(Path::new(path).starts_with(format!("/var/lib/{directory}")));
    }
    // Night sleep is a skill (#940): Skills' routines and Expression's rest
    // requests are on, each in its own unit only; the caps stay off.
    for (unit, text) in install::PRODUCT {
        for line in section(text, "Service") {
            for (switch, owner) in [
                ("LAMP_SKILL_ROUTINES", "lamp-skills.service"),
                ("LAMP_REST_REQUESTS", "lamp-expression.service"),
                ("LAMP_OUTPUT_CAPS", ""),
            ] {
                assert_eq!(
                    line == format!("Environment={switch}=on"),
                    line.contains(&format!("{switch}=on")),
                    "{unit}: {line}"
                );
                if line.contains(&format!("{switch}=on")) {
                    assert_eq!(*unit, owner, "{line}");
                }
            }
        }
    }
    for (owner, switch) in [
        ("lamp-skills.service", "LAMP_SKILL_ROUTINES"),
        ("lamp-expression.service", "LAMP_REST_REQUESTS"),
    ] {
        let (_, text) = install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == owner)
            .unwrap();
        assert!(
            section(text, "Service").contains(&format!("Environment={switch}=on").as_str()),
            "{owner}"
        );
    }
    // Health and crash reports are on in Cloud, which sends them only after
    // the person's yes, and in the voice, which can only stop them (#2862).
    // No other product unit sets the switch.
    for (unit, text) in install::PRODUCT {
        let on = section(text, "Service").contains(&"Environment=LAMP_REPORTS=on");
        let mentioned = section(text, "Service")
            .iter()
            .any(|line| line.contains("LAMP_REPORTS"));
        let owner = ["lamp-cloud.service", "lamp-voice.service"].contains(unit);
        assert_eq!(on, owner, "{unit}");
        assert_eq!(mentioned, owner, "{unit}");
    }
    // Actions needs no caps path while off, and the product's new argument
    // alone must not open persistent storage or opt into the cap service.
    let caps = Path::new("/var/lib/lampos-output-caps");
    let bus = Path::new("/run/lampos-bus");
    assert!(
        lamp_actions::caps_service::Config::configured(None, None, None)
            .unwrap()
            .is_none()
    );
    assert!(
        lamp_actions::caps_service::Config::configured(None, Some(caps), Some(bus))
            .unwrap()
            .is_none()
    );
    assert!(
        lamp_actions::caps_service::Config::configured(
            Some(std::ffi::OsStr::new("on")),
            None,
            Some(bus)
        )
        .is_err()
    );
}

/// An install takes the units from the updater's own build and the binaries
/// from the channel's newest, so a unit may meet an older binary. Today's
/// `lamp-actions` and `lamp-skills` exit 2 on an option they don't know, and
/// with `Restart=on-failure` both would loop (#2157). The product units pass
/// only the options both took before Bedtime and Morning (lamp-voice
/// `ff4a26099`); a newer setting comes as an `Environment=` line, which an
/// older binary ignores. A new option belongs here only once every channel
/// carries a binary that takes it.
#[test]
fn product_units_pass_only_options_an_older_binary_takes() {
    for (unit, known) in [
        (
            "lamp-actions.service",
            &[
                "--dir",
                "--mixer-card",
                "--mixer-control",
                "--volume-ceiling",
                "--bus-root",
            ][..],
        ),
        (
            "lamp-skills.service",
            &[
                "--dir",
                "--bus-root",
                "--gaze-socket",
                "--mood-socket",
                "--actions-socket",
                "--say-socket",
                "--tools-dir",
                "--state-dir",
                "--boot-id",
                "--clock-control",
                "--known-file",
                "--zone",
                "--utc-offset-minutes",
            ][..],
        ),
    ] {
        let text = install::PRODUCT
            .iter()
            .find(|(name, _)| *name == unit)
            .unwrap()
            .1;
        let command = section(text, "Service")
            .into_iter()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .unwrap();
        let options: Vec<_> = command
            .split_whitespace()
            .filter(|word| word.starts_with("--"))
            .collect();
        assert!(!options.is_empty(), "{unit}");
        for option in options {
            assert!(known.contains(&option), "{unit}: {option}");
        }
    }
}

/// The skills' tools were in the actions part's folder, which systemd
/// removes at each restart of actions, and the voice had no skill functions
/// until lamp-skills restarted (lamp-4ace, 19 Oct 04:33). The product's
/// skills keep them in a folder of their own, and its voice reads that one.
#[test]
fn the_skills_tools_are_in_their_own_folder_which_the_voice_reads() {
    let unit = |name: &str| {
        install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap()
            .1
    };
    let skills = unit("lamp-skills.service");
    assert!(skills.contains("\nRuntimeDirectory=lampos-skills\n"));
    // The skills a person turned off are kept across a restart in its own
    // state folder (#1685), the one #1732's daily journal shares.
    assert!(skills.contains(" --state-dir /var/lib/lampos-skills-state "));
    assert!(skills.contains("\nStateDirectory=lampos-skills-state\nStateDirectoryMode=0700\n"));
    assert!(skills.contains(" --tools-dir /run/lampos-skills "));
    assert!(!skills.contains("/run/lampos-actions/skills"));
    assert!(
        unit("lamp-voice.service").contains("\nEnvironment=LAMP_SKILLS_DIR=/run/lampos-skills\n")
    );
}

/// #1390: the product's tasks part closes the harness each task started once
/// its answer is told (`--close-told`), so a person's computer doesn't keep
/// an idle Claude Code session for every job the lamp ever handed over.
#[test]
fn the_tasks_part_closes_the_harness_a_told_task_started() {
    let tasks = install::PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-tasks.service")
        .unwrap()
        .1;
    let exec = section(tasks, "Service")
        .into_iter()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(exec.contains(" --close-told "), "{exec}");
    // As the person, as the product calls Harness: the agent id the lamp
    // kept is the only guard on what it closes (docs/tasks.md).
    assert!(exec.contains(" --harness-as-person "), "{exec}");
}

/// #811, from lamp-06c1: without Harness (its own installer's to put on the
/// lamp), the tasks part is skipped by its unit's condition rather than
/// failing ten times; the install says Harness is missing.
#[test]
fn the_tasks_part_waits_for_harness_rather_than_failing() {
    let tasks = install::PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-tasks.service")
        .unwrap()
        .1;
    let unit = section(tasks, "Unit");
    assert!(unit.contains(&format!("ConditionPathExists={}", install::HARNESS).as_str()));
    let exec = section(tasks, "Service")
        .into_iter()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(exec.contains(&format!("--harness {}", install::HARNESS)));
    // Health lists it as off, not missing, while Harness is not there.
    let health = install::PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-health.service")
        .unwrap()
        .1;
    assert!(health.contains(&format!(
        "--expect tasks --needs tasks={}",
        install::HARNESS
    )));
    // The voice says it isn't set up, not that a part failed, by the same file.
    let voice = install::PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-voice.service")
        .unwrap()
        .1;
    assert!(voice.contains(&format!(
        "Environment=LAMP_TASKS_NEEDS={}",
        install::HARNESS
    )));
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(
        lamp.said.iter().any(|line| line.contains("harness")),
        "{:?}",
        lamp.said
    );
}

fn sha(bytes: &[u8]) -> String {
    lamp_update::manifest::sha256_hex(bytes)
}

/// #811, from lamp-06c1: no build carries a model, so install fetches each
/// missing one and checks it by size and SHA-256 before it is placed, one
/// out of a pinned archive too. One that cannot be had, or does not match, is said
/// and leaves nothing; one already there and not the pinned one is kept.
#[test]
fn missing_models_are_fetched_checked_and_placed_and_a_lamps_own_is_kept() {
    let mut lamp = Lamp::new(188, &[]);
    let eyes = b"a face model".to_vec();
    let lib = b"a runtime library".to_vec();
    // A real gzipped tar holding the library, as the runtime's release is.
    let work = tempfile::tempdir().unwrap();
    fs::create_dir_all(work.path().join("rt/lib")).unwrap();
    fs::write(work.path().join("rt/lib/lib.so"), &lib).unwrap();
    let made = std::process::Command::new("tar")
        .arg("-czf")
        .arg(work.path().join("rt.tgz"))
        .arg("-C")
        .arg(work.path())
        .arg("rt/lib/lib.so")
        .status()
        .unwrap();
    assert!(made.success());
    let tgz = fs::read(work.path().join("rt.tgz")).unwrap();
    lamp.channel
        .files
        .insert("https://models.test/eyes.onnx".into(), eyes.clone());
    lamp.channel
        .files
        .insert("https://models.test/rt.tgz".into(), tgz.clone());
    lamp.channel
        .files
        .insert("https://models.test/bad.onnx".into(), b"tampered".to_vec());
    let model = |path: &str, url: &str, bytes: &[u8]| install::Model {
        path: path.into(),
        url: url.into(),
        bytes: bytes.len() as u64,
        sha256: sha(bytes),
        archive: None,
    };
    let mut runtime = model(
        "/opt/lampOS/voice/current/lib.so",
        "https://models.test/rt.tgz",
        &lib,
    );
    runtime.archive = Some(install::Archive {
        bytes: tgz.len() as u64,
        sha256: sha(&tgz),
        member: "rt/lib/lib.so".into(),
    });
    lamp.config.models = vec![
        model(
            "/opt/lampOS/eyes/models/eyes.onnx",
            "https://models.test/eyes.onnx",
            &eyes,
        ),
        runtime,
        model(
            "/opt/lampOS/eyes/models/bad.onnx",
            "https://models.test/bad.onnx",
            b"genuine",
        ),
        model(
            "/opt/lampOS/eyes/models/gone.onnx",
            "https://models.test/gone.onnx",
            b"x",
        ),
    ];
    lamp.install().unwrap();
    assert_eq!(
        fs::read(lamp.on("/opt/lampOS/eyes/models/eyes.onnx")).unwrap(),
        eyes
    );
    assert_eq!(
        fs::read(lamp.on("/opt/lampOS/voice/current/lib.so")).unwrap(),
        lib
    );
    assert!(
        fs::read_link(lamp.on("/opt/lampOS/voice/current")).is_ok(),
        "beside the binary"
    );
    for missing in [
        "bad.onnx",
        "gone.onnx",
        ".bad.onnx.fetching",
        ".gone.onnx.fetching",
    ] {
        assert!(
            !lamp
                .on(&format!("/opt/lampOS/eyes/models/{missing}"))
                .exists(),
            "{missing}"
        );
    }
    assert!(
        !lamp
            .on("/opt/lampOS/voice/current/.lib.so.archive")
            .exists()
    );
    for said in ["bad.onnx could not be had", "gone.onnx could not be had"] {
        assert!(
            lamp.said.iter().any(|line| line.contains(said)),
            "{said}: {:?}",
            lamp.said
        );
    }

    // Again: nothing new is fetched, and a model of the lamp's own is kept.
    fs::write(
        lamp.on("/opt/lampOS/eyes/models/eyes.onnx"),
        b"the lamp's own",
    )
    .unwrap();
    lamp.said.clear();
    let again = lamp.install().unwrap();
    assert!(
        again
            .changed
            .iter()
            .all(|line| !line.starts_with("fetched")),
        "{:?}",
        again.changed
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with("kept /opt/lampOS/eyes/models/eyes.onnx"))
    );

    // A new pin (the reviewer, thirteen): the earlier one this program
    // placed is replaced, since its part checks the new pin; the lamp's own
    // file is still kept.
    let newer = b"a runtime library, newer".to_vec();
    lamp.channel
        .files
        .insert("https://models.test/lib-2.so".into(), newer.clone());
    lamp.config.models[1] = model(
        "/opt/lampOS/voice/current/lib.so",
        "https://models.test/lib-2.so",
        &newer,
    );
    let bumped = lamp.install().unwrap();
    assert_eq!(
        fs::read(lamp.on("/opt/lampOS/voice/current/lib.so")).unwrap(),
        newer
    );
    assert!(
        bumped
            .changed
            .contains(&"fetched /opt/lampOS/voice/current/lib.so".to_owned())
    );
    assert_eq!(
        fs::read(lamp.on("/opt/lampOS/eyes/models/eyes.onnx")).unwrap(),
        b"the lamp's own"
    );
}

/// The pinned list itself: every model under `/opt/lampOS`, from our one
/// release, pinned by a whole SHA-256; the voice's two the ones the turn-end
/// worker checks, and `name.onnx` the one the lamps run (the first builder
/// on #811, 13:58).
#[test]
fn the_pinned_models_are_the_ones_their_parts_check() {
    let all = install::models().unwrap();
    let (notices, models): (Vec<_>, Vec<_>) = all
        .iter()
        .partition(|model| model.path.starts_with(NOTICES));
    assert_eq!((models.len(), notices.len()), (9, 5));
    let release = "https://github.com/autonomous-ai/lampOS/releases/download/models-2026-10-16/";
    for model in &all {
        assert!(model.path.starts_with("/opt/lampOS/"), "{}", model.path);
        assert_eq!(model.sha256.len(), 64);
        assert!(model.bytes > 0);
    }
    for model in &models {
        assert!(model.url.starts_with(release), "{}", model.url);
    }
    let wire = include_str!("../../turn-end-wire/src/lib.rs");
    let fast = include_str!("../../turn-end/src/fast.rs");
    let find = |name: &str| models.iter().find(|m| m.path.ends_with(name)).unwrap();
    assert!(wire.contains(&find("smart-turn-v3.2-gpu.onnx").sha256));
    let runtime = find("libonnxruntime.so.1.30.0");
    assert!(fast.contains(&runtime.sha256));
    assert_eq!(runtime.bytes, 25_135_496);
    assert!(fast.contains("bytes: 25_135_496"));
    let eyes = include_str!("../../../deploy/product/lamp-eyes.service");
    for name in [
        "face_detection_yunet_2023mar.onnx",
        "face_recognition_sface_2021dec.onnx",
    ] {
        assert!(
            eyes.contains(&format!("/opt/lampOS/eyes/models/{name}")),
            "{name}"
        );
        find(name);
    }
    assert!(
        include_str!("../../../deploy/product/lamp-name.service")
            .contains("--models /opt/lampOS/name/models")
    );
    // The mesh and Blendshape V2 (#1671, #2799): placed beside YuNet and
    // SFace at the eyes' own pins, and passed by the product's unit, so how
    // a face seems is read by Blendshape V2 on every lamp.
    for (name, pin) in [
        ("face_mesh_v2.onnx", include_str!("../../eyes/src/mesh.rs")),
        (
            "face_blendshapes.onnx",
            include_str!("../../eyes/src/blendshapes.rs"),
        ),
    ] {
        let model = find(name);
        assert_eq!(model.path, format!("/opt/lampOS/eyes/models/{name}"));
        assert!(pin.contains(&model.sha256), "{name}");
    }
    assert!(eyes.contains(
        "--mouth-model /opt/lampOS/eyes/models/face_mesh_v2.onnx --seems-blendshapes /opt/lampOS/eyes/models/face_blendshapes.onnx"
    ));
    assert_eq!(find("face_mesh_v2.onnx").bytes, 4_915_552);
    assert_eq!(find("face_blendshapes.onnx").bytes, 1_882_583);
    // FER+ stays out of the product (#2799): its training pictures are for
    // research only, so no lamp is given it or runs it, and the measure's
    // log is never on in the product.
    assert!(all.iter().all(|model| !model.path.contains("ferplus")));
    let start = eyes
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    for arg in start.split_whitespace() {
        assert!(
            !["--seems", "--seems-log"].contains(&arg) && !arg.contains("ferplus"),
            "{arg}"
        );
    }
    assert!(
        include_str!("../../../deploy/product/lamp-mood.service")
            .lines()
            .any(|line| line == "Environment=LAMP_MOOD_SEEMS=on")
    );
}

/// Music sway and watch you leave are core skills in the product (#2834):
/// installed on, out of `PRODUCT_SKILLS_OFF`, and the eyes say `room/stood_up`
/// (`LAMP_ACTIVITY=place`, by place only), which watch you leave reads.
/// Music sway reads the ears' sound classes, which the product turns on
/// separately, once YAMNet's weights may be shipped; until then it has
/// nothing to read and never sways. Both stay quiet at night by their own
/// `[quiet]`.
#[test]
fn music_sway_and_watch_you_leave_are_on_in_the_product() {
    let skills = ["music-sway", "watch-you-leave"];
    for skill in skills {
        assert!(install::PRODUCT_SKILLS.contains(&skill), "{skill}");
        assert!(
            !install::PRODUCT_SKILLS_OFF
                .iter()
                .any(|(off, _)| *off == skill),
            "{skill}"
        );
        let toml = fs::read_to_string(skills_root().join(skill).join("skill.toml")).unwrap();
        assert!(toml.contains("\n[quiet]"), "{skill} is quiet at night");
    }
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    for skill in skills {
        assert_eq!(
            fs::read(lamp.on(&format!("/var/lib/lampOS/skills/{skill}/skill.toml"))).unwrap(),
            fs::read(skills_root().join(skill).join("skill.toml")).unwrap(),
            "{skill} is installed as its own file, turned on"
        );
    }
    let set = |unit: &str, line: &str| unit.lines().any(|each| each.trim() == line);
    let eyes = include_str!("../../../deploy/product/lamp-eyes.service");
    assert!(set(eyes, "Environment=LAMP_BUS=on"));
    // By place only: the unit passes `--recogniser`, and `on` would tell
    // the sitter by face print, which waits for its privacy read (eyes.md).
    assert!(set(eyes, "Environment=LAMP_ACTIVITY=place"));
    assert!(eyes.contains("--recogniser"));
}

/// Where the product's voice finds its recorded lines (#1844).
const NOTICES: &str = "/opt/lampOS/voice/notices/";

/// #1844 (lamp-06c1, 20 Oct): the product unit named a notice folder
/// nothing filled, so the offline line and "back online" were silent. Now
/// install places the four recordings and their manifest, from their own
/// release, in the folder the product voice reads.
#[test]
fn install_places_the_voices_recorded_lines_where_the_product_voice_reads_them() {
    let all = install::models().unwrap();
    let release = "https://github.com/autonomous-ai/lampOS/releases/download/notices-2026-10-20/";
    let mut placed: Vec<&str> = all
        .iter()
        .filter_map(|model| model.path.strip_prefix(NOTICES))
        .collect();
    placed.sort_unstable();
    assert_eq!(
        placed,
        [
            "back-online.en-US.wav",
            "manifest.json",
            "provider-unavailable.en-US.wav",
            "voice-service-unreachable.en-US.wav",
            "wifi-lost.en-US.wav",
        ]
    );
    for model in all.iter().filter(|model| model.path.starts_with(NOTICES)) {
        let file = model.path.strip_prefix(NOTICES).unwrap();
        assert_eq!(model.url, format!("{release}{file}"));
        assert!(model.archive.is_none(), "{file}");
    }
    let voice = include_str!("../../../deploy/product/lamp-voice.service");
    let start = voice
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(
        start.contains(&format!(
            " --failure-notice-dir {} --failure-notice-language en-US",
            NOTICES.trim_end_matches('/')
        )),
        "{start}"
    );
}

/// #2943: a lamp moved from V1 gets no `lamp-update install` and its
/// package carries no notices, so `lamp-keeper prepare` places the same
/// pinned lines, by install's own fetch and check, in the folder the
/// generic voice unit names. Only a missing line is fetched: one at its pin
/// is left, one that isn't is kept and said, and one that can't be had is
/// said while the rest are still placed. Nothing is refused, and nothing
/// goes under `/opt/lampOS`.
#[test]
fn prepare_places_the_recorded_lines_a_moved_lamps_voice_reads() {
    let root = tempfile::tempdir().unwrap();
    let at = |name: &str| {
        root.path()
            .join(install::NOTICES_MOVED.trim_start_matches('/'))
            .join(name)
    };
    let line = |name: &str, bytes: &[u8]| manifest::Model {
        path: format!("{NOTICES}{name}"),
        url: format!("https://release.test/{name}"),
        bytes: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        archive: None,
    };
    let models = [
        line("manifest.json", b"{\"lines\":1}"),
        line("provider-unavailable.en-US.wav", b"the service's line"),
        line("back-online.en-US.wav", b"back online"),
        line("wifi-lost.en-US.wav", b"no wifi"),
        line("voice-service-unreachable.en-US.wav", b"unreachable"),
        // Not a notice: never placed by prepare.
        manifest::Model {
            path: "/opt/lampOS/eyes/models/eyes.onnx".into(),
            ..line("eyes.onnx", b"eyes")
        },
    ];
    let mut channel = FakeChannel::default();
    for (name, bytes) in [
        ("manifest.json", &b"{\"lines\":1}"[..]),
        ("provider-unavailable.en-US.wav", b"the service's line"),
        ("back-online.en-US.wav", b"back online"),
        ("wifi-lost.en-US.wav", b"no wifi"),
        ("eyes.onnx", b"eyes"),
    ] {
        channel
            .files
            .insert(format!("https://release.test/{name}"), bytes.to_vec());
    }
    // One already at its pin, one the lamp's own.
    fs::create_dir_all(at("")).unwrap();
    fs::write(at("back-online.en-US.wav"), b"back online").unwrap();
    fs::write(at("wifi-lost.en-US.wav"), b"a line of its own").unwrap();
    let mut said = Vec::new();
    let wrote = install::place_moved_notices_of(&models, root.path(), &mut channel, &mut |line| {
        said.push(line.to_owned())
    });
    let moved = |name: &str| format!("{}/{name}", install::NOTICES_MOVED);
    assert_eq!(
        wrote,
        [
            moved("manifest.json"),
            moved("provider-unavailable.en-US.wav")
        ]
    );
    assert_eq!(fs::read(at("manifest.json")).unwrap(), b"{\"lines\":1}");
    assert_eq!(
        fs::read(at("provider-unavailable.en-US.wav")).unwrap(),
        b"the service's line"
    );
    assert_eq!(
        fs::read(at("wifi-lost.en-US.wav")).unwrap(),
        b"a line of its own"
    );
    assert!(!at("voice-service-unreachable.en-US.wav").exists());
    assert!(!root.path().join("opt").exists());
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(at("manifest.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said[0].starts_with(&format!("kept {}", moved("wifi-lost.en-US.wav")))
            && said[0].ends_with("remove it and run prepare again to take the pin"),
        "{said:?}"
    );
    assert!(
        said[1].starts_with(&format!(
            "{} could not be had",
            moved("voice-service-unreachable.en-US.wav")
        )) && said[1].contains("sigh"),
        "{said:?}"
    );
    // Again: everything there is left, and only the missing one is tried.
    said.clear();
    let wrote = install::place_moved_notices_of(&models, root.path(), &mut channel, &mut |line| {
        said.push(line.to_owned())
    });
    assert!(wrote.is_empty(), "{wrote:?}");
    assert_eq!(said.len(), 2, "{said:?}");
}

/// The pinned lines `lamp-keeper prepare` places are install's own: the
/// five under the product's folder, nothing else.
#[test]
fn a_moved_lamp_is_given_the_product_voices_five_lines() {
    let root = tempfile::tempdir().unwrap();
    let mut channel = FakeChannel::default();
    let mut said = Vec::new();
    let wrote = install::place_moved_notices(root.path(), &mut channel, &mut |line| {
        said.push(line.to_owned())
    });
    assert!(wrote.is_empty());
    assert_eq!(said.len(), 5, "{said:?}");
    for line in &said {
        assert!(line.starts_with(install::NOTICES_MOVED), "{line}");
    }
}

const PINS: &str = "/etc/lampOS/controls/controls-pins.json";

/// The reviewer's P1 on #2143: a lamp whose pins are its own (the
/// factory's head pads, before this build) keeps them, and gains the
/// build's `privacy` section, which the product's units now select; without
/// it Controls has no privacy source and the lamp never hears. Everything
/// else is kept, it is said, and installing again changes nothing. A pins
/// file that isn't a JSON object is left, and said.
#[test]
fn a_lamps_own_pins_gain_the_privacy_section_and_keep_the_rest() {
    let mut lamp = Lamp::new(188, &[]);
    let own = r#"{
  "chip": "/dev/gpiochip0",
  "buttons": [{"name": "primary", "line": 99, "active_low": true, "pull": "up"}],
  "head_pads": [{"name": "head-1", "line": 41, "active_low": true, "pull": "up"}],
  "every_ms": 5,
  "reset": {"name": "reset", "line": 100, "active_low": true, "pull": "up"},
  "legacy_head": true
}
"#;
    fs::create_dir_all(lamp.on(PINS).parent().unwrap()).unwrap();
    fs::write(lamp.on(PINS), own).unwrap();
    let first = lamp.install().unwrap();
    let pins: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(lamp.on(PINS)).unwrap()).unwrap();
    assert_eq!(
        pins["privacy"],
        serde_json::json!({"chip": "/dev/gpiochip1", "line": 9, "active_low": true, "pull": "up"})
    );
    let mut kept = pins.clone();
    kept.as_object_mut().unwrap().remove("privacy");
    // Its `legacy_head` goes too, as the build's pins have none (#711).
    let mut own_now = serde_json::from_str::<serde_json::Value>(own).unwrap();
    own_now.as_object_mut().unwrap().remove("legacy_head");
    assert_eq!(kept, own_now);
    assert!(
        lamp.said.iter().any(|line| line.starts_with(&format!(
            "took the old engine's head (legacy_head) out of {PINS}"
        ))),
        "{:?}",
        lamp.said
    );
    assert!(
        first.changed.contains(&format!("wrote {PINS}")),
        "{:?}",
        first.changed
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("added the privacy section to {PINS}"))),
        "{:?}",
        lamp.said
    );
    let added = fs::read_to_string(lamp.on(PINS)).unwrap();
    lamp.said.clear();
    let again = lamp.install().unwrap();
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), added);
    assert!(
        !again.changed.contains(&format!("wrote {PINS}")),
        "{:?}",
        again.changed
    );
    // Not a JSON object: left as it is, and said.
    let mut odd = Lamp::new(189, &[]);
    fs::create_dir_all(odd.on(PINS).parent().unwrap()).unwrap();
    fs::write(odd.on(PINS), "[]\n").unwrap();
    odd.install().unwrap();
    assert_eq!(fs::read_to_string(odd.on(PINS)).unwrap(), "[]\n");
    assert!(
        odd.said
            .iter()
            .any(|line| line.starts_with(&format!("{PINS} is not a JSON object"))),
        "{:?}",
        odd.said
    );
}

const CONTROLS: &str = "/etc/lampOS/controls/controls.json";

/// #1798: a lamp's own pins file still naming the old guess, head pads on
/// lines 37 and 96 (lamp-52e6's and lamp-cb94's, which read no touch on
/// them), gets the build's: every line a pad is wired to. Its own Controls
/// settings, naming two pads, name the same four, so a Controls put back
/// from before this, which wants both alike, still starts. Everything else
/// is kept, and installing again changes nothing. Pins the factory step
/// wrote (with each pad's baseline) and a settings file naming other pads
/// are kept.
#[test]
fn a_lamps_own_pins_on_the_old_guess_get_every_head_line() {
    let product: serde_json::Value =
        serde_json::from_str(SETTINGS.iter().find(|(path, _)| *path == PINS).unwrap().1).unwrap();
    let lines: Vec<_> = product["head_pads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pad| pad["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, [37, 96, 97, 98]);
    let guessed = r#"{
  "chip": "/dev/gpiochip0",
  "buttons": [{"name": "primary", "line": 99, "active_low": true, "pull": "up"}],
  "head_pads": [
    {"name": "head-1", "line": 37, "active_low": false, "pull": "down"},
    {"name": "head-2", "line": 96, "active_low": false, "pull": "down"}
  ],
  "every_ms": 5,
  "reset": {"name": "reset", "line": 100, "active_low": true, "pull": "up"},
  "legacy_head": true,
  "privacy": {"chip": "/dev/gpiochip1", "line": 9, "active_low": true, "pull": "up"}
}
"#;
    let own_controls = r#"{"enabled": true, "buttons": ["primary"], "head_pads": ["head-1", "head-2"], "base_selected_mask": null}
"#;
    let read = |lamp: &Lamp, path: &str| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(lamp.on(path)).unwrap()).unwrap()
    };
    let mut lamp = Lamp::new(190, &[]);
    fs::create_dir_all(lamp.on(PINS).parent().unwrap()).unwrap();
    fs::write(lamp.on(PINS), guessed).unwrap();
    fs::write(lamp.on(CONTROLS), own_controls).unwrap();
    let first = lamp.install().unwrap();
    let mut pins = read(&lamp, PINS);
    assert_eq!(pins["head_pads"], product["head_pads"]);
    pins["head_pads"] =
        serde_json::from_str::<serde_json::Value>(guessed).unwrap()["head_pads"].clone();
    // And no `legacy_head`, as the build's pins have none (#711).
    let mut guessed_now = serde_json::from_str::<serde_json::Value>(guessed).unwrap();
    guessed_now.as_object_mut().unwrap().remove("legacy_head");
    assert_eq!(pins, guessed_now);
    let mut controls = read(&lamp, CONTROLS);
    assert_eq!(
        controls["head_pads"],
        serde_json::json!(["head-1", "head-2", "head-3", "head-4"])
    );
    controls["head_pads"] = serde_json::json!(["head-1", "head-2"]);
    assert_eq!(
        controls,
        serde_json::from_str::<serde_json::Value>(own_controls).unwrap()
    );
    for path in [PINS, CONTROLS] {
        assert!(
            first.changed.contains(&format!("wrote {path}")),
            "{:?}",
            first.changed
        );
    }
    for said in [
        format!("put the build's head pads, every line a pad is wired to, in {PINS}"),
        format!("named the same head pads in {CONTROLS}"),
    ] {
        assert!(
            lamp.said.iter().any(|line| line.starts_with(&said)),
            "{said}: {:?}",
            lamp.said
        );
    }
    // Controls takes the two files together.
    let wiring: lamp_controls::touch_pins::TouchPinsConfig =
        serde_json::from_value(read(&lamp, PINS)).unwrap();
    let settings: lamp_controls::engine::Config =
        serde_json::from_value(read(&lamp, CONTROLS)).unwrap();
    wiring.check(&settings).unwrap();
    let (pins, controls) = (
        fs::read_to_string(lamp.on(PINS)).unwrap(),
        fs::read_to_string(lamp.on(CONTROLS)).unwrap(),
    );
    let again = lamp.install().unwrap();
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), pins);
    assert_eq!(fs::read_to_string(lamp.on(CONTROLS)).unwrap(), controls);
    assert!(
        !again
            .changed
            .iter()
            .any(|line| line.starts_with("wrote /etc/lampOS/controls/")),
        "{:?}",
        again.changed
    );

    // The factory step's pins, the same lines with each pad's baseline, are
    // this unit's own and stay; so do settings naming other pads.
    let factory = guessed.replace(
        "  \"every_ms\": 5,",
        "  \"every_ms\": 5,\n  \"head_rest\": [{\"name\": \"head-1\", \"rest\": \"low\", \"reads\": \"level\"}, {\"name\": \"head-2\", \"rest\": \"low\", \"reads\": \"level\"}],",
    );
    assert_ne!(factory, guessed);
    let other_controls = own_controls.replace("\"head-2\"]", "\"head-9\"]");
    let mut found = Lamp::new(191, &[]);
    fs::create_dir_all(found.on(PINS).parent().unwrap()).unwrap();
    fs::write(found.on(PINS), &factory).unwrap();
    fs::write(found.on(CONTROLS), &other_controls).unwrap();
    found.install().unwrap();
    // Its head pads and baseline stay; only `legacy_head` goes (#711).
    let mut factory_now = serde_json::from_str::<serde_json::Value>(&factory).unwrap();
    factory_now.as_object_mut().unwrap().remove("legacy_head");
    assert_eq!(read(&found, PINS), factory_now);
    assert_eq!(
        fs::read_to_string(found.on(CONTROLS)).unwrap(),
        other_controls
    );
}

/// The reviewer's case on #815: install, move the primary button to
/// another line on this lamp, install the same build again. The lamp's
/// line stays, it is said, and nothing else is written or asked.
#[test]
fn a_setting_changed_on_the_lamp_is_kept_when_install_runs_again() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    let pins = fs::read_to_string(lamp.on(PINS)).unwrap();
    let ours = pins.replace("\"line\": 99", "\"line\": 100");
    assert_ne!(ours, pins);
    let parsed: serde_json::Value = serde_json::from_str(&ours).unwrap();
    assert_eq!(parsed["buttons"][0]["line"], 100);
    fs::write(lamp.on(PINS), &ours).unwrap();
    let before = tree(&lamp.root);
    let calls = lamp.host.calls.len();
    lamp.said.clear();

    let again = lamp.install().unwrap();
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), ours);
    assert_eq!(again.changed, Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);
    assert_eq!(lamp.host.calls.len(), calls, "nothing asked of the machine");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {PINS}"))),
        "{:?}",
        lamp.said
    );
}

/// lamp-cb94 (the first builder on #705, 12:13): its settings were there
/// before any install, install's own bytes but closed (0600), so the parts
/// run as `lampos` never read them and Controls and Expression failed.
/// Install takes such a default for its own and opens it; a lamp's own
/// closed setting is kept as it is, and said.
#[test]
fn a_default_found_closed_is_opened_and_a_lamps_own_found_closed_is_said() {
    use std::os::unix::fs::PermissionsExt;
    const CONTROLS: &str = "/etc/lampOS/controls/controls.json";
    let mut lamp = Lamp::new(188, &[]);
    let default = SETTINGS.iter().find(|(path, _)| *path == PINS).unwrap().1;
    let own = "{\"the lamp's\": \"own\"}\n";
    // A product unit with the build's bytes, closed: `put()`'s equal bytes.
    let (name, text) = PRODUCT[0];
    let unit = format!("{}{name}", install::UNITS);
    // A unit linked to the same bytes elsewhere: the link's target is left.
    let (linked_name, linked_text) = PRODUCT[1];
    let linked = format!("{}{linked_name}", install::UNITS);
    let target = lamp.root.join("elsewhere");
    fs::write(&target, linked_text).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    fs::create_dir_all(lamp.on(install::UNITS)).unwrap();
    std::os::unix::fs::symlink(&target, lamp.on(&linked)).unwrap();
    fs::create_dir_all(lamp.on("/etc/lampOS/controls")).unwrap();
    for (path, text) in [(PINS, default), (CONTROLS, own), (unit.as_str(), text)] {
        fs::write(lamp.on(path), text).unwrap();
        fs::set_permissions(lamp.on(path), fs::Permissions::from_mode(0o600)).unwrap();
    }

    let first = lamp.install().unwrap();
    assert_eq!(mode_of(&lamp.on(PINS)), 0o644);
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), default);
    assert_eq!(lamp.state().settings[PINS], sha256_hex(default.as_bytes()));
    assert!(
        first.changed.contains(&format!("made {PINS} 644, was 600")),
        "{:?}",
        first.changed
    );
    assert_eq!(mode_of(&lamp.on(&unit)), 0o644);
    assert!(first.changed.contains(&format!("made {unit} 644, was 600")));
    assert!(fs::symlink_metadata(lamp.on(&linked)).unwrap().is_symlink());
    assert_eq!(mode_of(&target), 0o600);
    assert_eq!(mode_of(&lamp.on(CONTROLS)), 0o600);
    assert_eq!(fs::read_to_string(lamp.on(CONTROLS)).unwrap(), own);
    let closed = format!("{CONTROLS} is closed to other users (600)");
    assert!(
        lamp.said.iter().any(|line| line.starts_with(&closed)),
        "{:?}",
        lamp.said
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains(&format!("{PINS} is closed"))),
        "{:?}",
        lamp.said
    );

    // Once open, nothing more to do; the lamp's own is still said.
    lamp.said.clear();
    let again = lamp.install().unwrap();
    assert_eq!(again.changed, Vec::<String>::new());
    assert!(lamp.said.iter().any(|line| line.starts_with(&closed)));
}

/// The private half (the reviewer on #1293): the device key found with
/// V1's key but open (0644) is narrowed to 0600, and the key is in no line
/// said or changed.
#[test]
fn a_device_key_found_open_is_narrowed_and_never_said() {
    use std::os::unix::fs::PermissionsExt;
    const KEY: &str = "dev-key-0123456789abcdef";
    let mut lamp = Lamp::new(188, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(
        lamp.on(install::V1_CONFIG),
        format!(r#"{{"llm_api_key": "{KEY}", "device_id": "lamp-06c1"}}"#),
    )
    .unwrap();
    let credential = install::CREDENTIAL_PATH;
    fs::create_dir_all(lamp.on(credential).parent().unwrap()).unwrap();
    fs::write(lamp.on(credential), KEY).unwrap();
    fs::set_permissions(lamp.on(credential), fs::Permissions::from_mode(0o644)).unwrap();

    let first = lamp.install().unwrap();
    assert_eq!(fs::read_to_string(lamp.on(credential)).unwrap(), KEY);
    assert_eq!(mode_of(&lamp.on(credential)), 0o600);
    assert!(
        first
            .changed
            .contains(&format!("made {credential} 600, was 644")),
        "{:?}",
        first.changed
    );
    for line in first.changed.iter().chain(&lamp.said) {
        assert!(!line.contains(KEY), "{line}");
    }
}

/// #811: `--setup on` writes setup's two settings, exactly the setup crate's
/// own-page and product reset files, so `lamp-setup.service` starts on a
/// blank lamp with no file by hand. Without it, none is written. A serve
/// file changed on the lamp is kept as any setting is.
#[test]
fn setup_settings_are_written_only_with_setup_on_and_kept_once_changed() {
    let serve = "/etc/lampOS/setup/serve.json";
    let reset = "/etc/lampOS/setup/reset-plan.json";
    let mut off = Lamp::new(188, &[]);
    off.install().unwrap();
    assert!(!off.on(serve).exists() && !off.on(reset).exists());

    let mut lamp = Lamp::new(188, &[]);
    lamp.config.setup = true;
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(serve)).unwrap(),
        include_str!("../../setup/fixtures/serve-own-page.json")
    );
    let plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(lamp.on(reset)).unwrap()).unwrap();
    assert_eq!(
        plan["stop"],
        serde_json::json!(["/usr/bin/systemctl", "stop", "lampos.target"])
    );
    let before = tree(&lamp.root);
    assert_eq!(lamp.install().unwrap().changed, Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);

    let ours = fs::read_to_string(lamp.on(serve))
        .unwrap()
        .replace("\"US\"", "\"SG\"");
    fs::write(lamp.on(serve), &ours).unwrap();
    lamp.said.clear();
    assert_eq!(lamp.install().unwrap().changed, Vec::<String>::new());
    assert_eq!(fs::read_to_string(lamp.on(serve)).unwrap(), ours);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {serve}")))
    );
}

/// #811, found on lamp-06c1: the lock folders are what the body and the eyes
/// accept (each 0700 and the parts' own, under a parent nobody else may
/// write), and the voice's sessions folder, which its unit names and nothing
/// else makes, is there.
#[test]
fn tmpfiles_make_lock_folders_the_parts_accept_and_the_voices_sessions_folder() {
    let lines: Vec<Vec<&str>> = install::TMPFILES
        .lines()
        .filter(|line| line.starts_with("d "))
        .map(|line| line.split_whitespace().collect())
        .collect();
    let line = |path: &str| {
        lines
            .iter()
            .find(|fields| fields[1] == path)
            .unwrap_or_else(|| panic!("no tmpfiles line for {path}"))
            .clone()
    };
    let parent = line("/run/lampos-locks");
    assert_eq!(u32::from_str_radix(parent[2], 8).unwrap() & 0o022, 0);
    assert_eq!(parent[3], "root");
    for lock in ["/run/lampos-locks/motor", "/run/lampos-locks/camera"] {
        assert_eq!(line(lock)[2..4], ["0700", install::USER]);
    }
    assert_eq!(
        line("/var/lib/lampOS/voice/sessions")[2..4],
        ["0700", "root"]
    );
    let voice = install::PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-voice.service")
        .unwrap()
        .1;
    assert!(voice.contains("/var/lib/lampOS/voice/sessions/${INVOCATION_ID}"));
}

/// The reviewer on #1913, 19:50: the tmpfiles install writes are recorded,
/// so a build's newer tmpfiles reach the lamp.
#[test]
fn install_records_the_tmpfiles_it_writes() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert_eq!(
        lamp.state().settings[install::TMPFILES_PATH],
        sha256_hex(install::TMPFILES.as_bytes())
    );
}

/// A newer build's default reaches a setting nobody changed; a setting
/// changed on the lamp, or there before any install, keeps the lamp's
/// bytes; a setting that is missing is written.
#[test]
fn a_newer_default_replaces_only_a_setting_nobody_changed() {
    let lamp = Lamp::new(188, &[]);
    let (untouched, changed_here, there_before, missing) = (
        "/etc/lampOS/a/untouched.json",
        "/etc/lampOS/a/changed.json",
        "/etc/lampOS/a/before.json",
        "/etc/lampOS/a/missing.json",
    );
    fs::create_dir_all(lamp.on("/etc/lampOS/a")).unwrap();
    fs::write(lamp.on(there_before), "the lamp's own\n").unwrap();
    let mut written = State::default();
    let mut said = Vec::new();
    let mut run = |defaults: &[(&str, &str)], written: &mut State| {
        let mut changed = Vec::new();
        install::settings(
            &lamp.config,
            defaults,
            written,
            &mut changed,
            &mut |line| said.push(line.to_owned()),
            &mut |_| Ok(()),
        )
        .unwrap();
        changed
    };

    let first = run(
        &[
            (untouched, "one\n"),
            (changed_here, "one\n"),
            (there_before, "one\n"),
        ],
        &mut written,
    );
    assert_eq!(
        first,
        [
            format!("wrote {untouched}"),
            format!("wrote {changed_here}")
        ]
    );
    assert!(!written.settings.contains_key(there_before));
    fs::write(lamp.on(changed_here), "mine\n").unwrap();

    let second = run(
        &[
            (untouched, "two\n"),
            (changed_here, "two\n"),
            (there_before, "two\n"),
            (missing, "two\n"),
        ],
        &mut written,
    );
    assert_eq!(
        second,
        [format!("wrote {untouched}"), format!("wrote {missing}")]
    );
    let read = |path: &str| fs::read_to_string(lamp.on(path)).unwrap();
    assert_eq!(read(untouched), "two\n");
    assert_eq!(read(changed_here), "mine\n");
    assert_eq!(read(there_before), "the lamp's own\n");
    assert_eq!(read(missing), "two\n");
    assert_eq!(written.settings[untouched], sha256_hex(b"two\n"));
    assert_eq!(
        written.settings[changed_here],
        sha256_hex(b"one\n"),
        "left as it was"
    );

    // Set back to the default by hand, it is the default's again, and the
    // next build's default replaces it.
    fs::write(lamp.on(changed_here), "two\n").unwrap();
    assert!(run(&[(changed_here, "two\n")], &mut written).is_empty());
    assert_eq!(written.settings[changed_here], sha256_hex(b"two\n"));
    assert_eq!(
        run(&[(changed_here, "three\n")], &mut written),
        [format!("wrote {changed_here}")]
    );
    assert_eq!(read(changed_here), "three\n");

    // Said each time one was kept: the one there before, twice, and the
    // changed one once; never the others.
    let kept: Vec<&String> = said
        .iter()
        .filter(|line| line.starts_with("kept "))
        .collect();
    assert_eq!(kept.len(), 3, "{said:?}");
    assert_eq!(
        kept.iter()
            .filter(|line| line.contains(there_before))
            .count(),
        2
    );
    assert_eq!(
        kept.iter()
            .filter(|line| line.contains(changed_here))
            .count(),
        1
    );
}

impl Lamp {
    fn state(&self) -> State {
        Records::new(self.on("/var/lib/lampos-update")).peek_state()
    }
}

/// The reviewer on #815 (19:17): an install whose `systemctl enable` failed
/// left every file as it should be, so the next run changed nothing and
/// never enabled the lamp; and the settings it wrote were not recorded, so a
/// later build's default took them for the lamp's own. Now what it wrote is
/// recorded before systemd is asked, the debt with it, and the next run pays
/// it once.
#[test]
fn an_install_whose_enable_failed_enables_at_the_next_run_and_keeps_its_settings_record() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.host.failing_enables = 1;
    let why = lamp.install().unwrap_err();
    assert!(why.contains("Failed to connect to bus"), "{why}");
    let state = lamp.state();
    assert!(state.systemd_owed);
    assert_eq!(state.installed.as_ref().map(|b| b.sequence), Some(188));
    for (path, text) in SETTINGS {
        assert_eq!(state.settings[*path], sha256_hex(text.as_bytes()), "{path}");
    }

    let calls = lamp.host.calls.len();
    let again = lamp.install().unwrap();
    assert_eq!(again.changed.len(), 1, "{:?}", again.changed);
    assert!(
        again.changed[0].starts_with("enabled "),
        "{:?}",
        again.changed
    );
    let asked: Vec<&str> = lamp.host.calls[calls..]
        .iter()
        .map(|call| call.split(' ').next().unwrap())
        .collect();
    assert_eq!(asked, ["tmpfiles", "enable"]);
    assert!(!lamp.state().systemd_owed);

    // Paid once: a third run asks nothing.
    let calls = lamp.host.calls.len();
    assert!(lamp.install().unwrap().changed.is_empty());
    assert_eq!(lamp.host.calls.len(), calls);

    // The default the failed run wrote is still known as install's: a later
    // build's default replaces it.
    let (path, _) = SETTINGS[2];
    let mut written = lamp.state();
    let mut changed = Vec::new();
    install::settings(
        &lamp.config,
        &[(path, "{}\n")],
        &mut written,
        &mut changed,
        &mut |_| {},
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(changed, [format!("wrote {path}")]);
}

/// A setting that cannot be read stops the install before any setting is
/// written; out of the way, the next run writes and records them all.
#[test]
fn a_setting_in_the_way_stops_the_install_before_any_is_written() {
    let mut lamp = Lamp::new(188, &[]);
    let (last, _) = SETTINGS[SETTINGS.len() - 1];
    // A folder where the file goes: it cannot be read or written over.
    fs::create_dir_all(lamp.on(last).join("in-the-way")).unwrap();
    let why = lamp.install().unwrap_err();
    assert!(why.starts_with(last), "{why}");
    for (path, _) in &SETTINGS[..SETTINGS.len() - 1] {
        assert!(!lamp.on(path).exists(), "{path}");
    }

    fs::remove_dir_all(lamp.on(last)).unwrap();
    lamp.install().unwrap();
    let state = lamp.state();
    assert!(state.settings_writing.is_empty(), "{state:?}");
    for (path, text) in SETTINGS {
        assert_eq!(state.settings[*path], sha256_hex(text.as_bytes()), "{path}");
    }
}

/// A later setting's write that fails leaves the earlier ones written and
/// still known as install's, from what was recorded before the first write:
/// a newer default replaces them.
#[test]
fn settings_written_before_one_that_fails_are_still_installs() {
    let lamp = Lamp::new(188, &[]);
    let (first, second) = ("/etc/lampOS/a/first.json", "/etc/lampOS/a/second.json");
    let mut state = State::default();
    let mut saved: Vec<State> = Vec::new();
    let failed = install::settings(
        &lamp.config,
        &[(first, "one\n"), (second, "one\n")],
        &mut state,
        &mut Vec::new(),
        &mut |_| {},
        &mut |state| {
            saved.push(state.clone());
            // Once the intent is saved, something takes the second's place.
            fs::create_dir_all(lamp.on(second).join("in-the-way")).unwrap();
            Ok(())
        },
    );
    assert!(failed.unwrap_err().starts_with(second));
    assert_eq!(fs::read_to_string(lamp.on(first)).unwrap(), "one\n");
    // What is on disk is the intent: the final record was never made.
    assert_eq!(saved.len(), 1);
    let mut on_disk = saved.remove(0);
    assert!(!on_disk.settings.contains_key(first));

    fs::remove_dir_all(lamp.on(second)).unwrap();
    let mut changed = Vec::new();
    install::settings(
        &lamp.config,
        &[(first, "two\n"), (second, "two\n")],
        &mut on_disk,
        &mut changed,
        &mut |line| panic!("{line}"),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        changed,
        [format!("wrote {first}"), format!("wrote {second}")]
    );
    assert!(on_disk.settings_writing.is_empty());
}

/// The reviewer on #815 (19:32): an install stopped after its units,
/// settings and zone were written but before it saved anything else (there,
/// at its "the body waits" line) left no debt, and the next run asked
/// systemd nothing. The debt is now saved before the first unit is written.
#[test]
fn an_install_stopped_after_writing_its_units_enables_at_the_next_run() {
    let mut lamp = Lamp::new(188, &[]);
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        install::install(
            &lamp.config,
            &mut lamp.channel,
            &mut lamp.host,
            &mut |line: &str| {
                if line.starts_with("the body waits") {
                    panic!("stopped here");
                }
            },
        )
    }));
    assert!(stopped.is_err());
    assert!(lamp.on("/etc/systemd/system/lamp-ears.service").exists());
    assert!(lamp.state().systemd_owed);
    assert!(
        lamp.host
            .calls
            .iter()
            .all(|call| !call.starts_with("enable"))
    );

    let again = lamp.install().unwrap();
    assert!(
        again
            .changed
            .iter()
            .any(|line| line.starts_with("enabled ")),
        "{:?}",
        again.changed
    );
    assert!(!lamp.state().systemd_owed);
}

/// A run stopped between writing a setting and recording it: the next
/// run, with a newer default, still knows the file as install's and
/// replaces it, from what was recorded before the write.
#[test]
fn a_setting_written_but_not_yet_recorded_is_still_installs() {
    let lamp = Lamp::new(188, &[]);
    let path = "/etc/lampOS/a/setting.json";
    let mut state = State::default();
    let mut saved: Vec<State> = Vec::new();
    // The first save is the intent; the run is stopped at the second, after
    // the write: what is on disk is the first.
    let stopped = install::settings(
        &lamp.config,
        &[(path, "ninety-nine\n")],
        &mut state,
        &mut Vec::new(),
        &mut |_| {},
        &mut |state| {
            saved.push(state.clone());
            if saved.len() == 2 {
                Err("stopped".into())
            } else {
                Ok(())
            }
        },
    );
    assert!(stopped.is_err());
    assert_eq!(fs::read_to_string(lamp.on(path)).unwrap(), "ninety-nine\n");
    let mut on_disk = saved[0].clone();
    assert!(!on_disk.settings.contains_key(path));

    let mut changed = Vec::new();
    install::settings(
        &lamp.config,
        &[(path, "one hundred\n")],
        &mut on_disk,
        &mut changed,
        &mut |line| panic!("{line}"),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(changed, [format!("wrote {path}")]);
    assert_eq!(on_disk.settings[path], sha256_hex(b"one hundred\n"));
    assert!(on_disk.settings_writing.is_empty());
}

/// The reviewer on #815 (21:42): the first run writes one setting and fails
/// on the next; the second, with newer defaults, fails on its first write;
/// the third must still know the first file as install's. Each run's own
/// record is what it saved before it failed.
#[test]
fn two_failed_runs_in_a_row_keep_a_written_setting_as_installs() {
    let lamp = Lamp::new(188, &[]);
    let (first, second) = ("/etc/lampOS/a/first.json", "/etc/lampOS/a/second.json");
    // A folder where a file's write goes beside it: that write fails, and
    // the file itself is still there to read.
    let block = |path: &str| {
        let name = Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let beside = lamp.on(path).with_file_name(format!(".{name}.lamp-update"));
        fs::create_dir_all(beside.join("in-the-way")).unwrap();
        beside
    };
    let run = |defaults: &[(&str, &str)], on_disk: &State, blocked: &str| {
        let mut state = on_disk.clone();
        let mut saved: Vec<State> = Vec::new();
        let result = install::settings(
            &lamp.config,
            defaults,
            &mut state,
            &mut Vec::new(),
            &mut |_| {},
            &mut |state| {
                saved.push(state.clone());
                if saved.len() == 1 && !blocked.is_empty() {
                    block(blocked);
                }
                Ok(())
            },
        );
        (
            result,
            saved.last().cloned().unwrap_or_else(|| on_disk.clone()),
        )
    };

    let (result, on_disk) = run(
        &[(first, "99\n"), (second, "99\n")],
        &State::default(),
        second,
    );
    assert!(result.unwrap_err().starts_with(second));
    assert_eq!(fs::read_to_string(lamp.on(first)).unwrap(), "99\n");
    fs::remove_dir_all(lamp.on("/etc/lampOS/a/.second.json.lamp-update")).unwrap();

    let (result, on_disk) = run(&[(first, "100\n"), (second, "100\n")], &on_disk, first);
    assert!(result.unwrap_err().starts_with(first));
    assert_eq!(fs::read_to_string(lamp.on(first)).unwrap(), "99\n");
    fs::remove_dir_all(lamp.on("/etc/lampOS/a/.first.json.lamp-update")).unwrap();

    let mut said = Vec::new();
    let mut state = on_disk;
    let mut changed = Vec::new();
    install::settings(
        &lamp.config,
        &[(first, "100\n"), (second, "100\n")],
        &mut state,
        &mut changed,
        &mut |line| said.push(line.to_owned()),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(said, Vec::<String>::new(), "nothing kept as the lamp's own");
    assert_eq!(
        changed,
        [format!("wrote {first}"), format!("wrote {second}")]
    );
    assert_eq!(fs::read_to_string(lamp.on(first)).unwrap(), "100\n");
    assert!(state.settings_writing.is_empty());
}

/// #811, the first builder's 01:56 recipe: with Harness on the lamp, install
/// sets it to start at boot as `lampos` once; without it, it says so and
/// goes on; a refusal stops the install and is said.
#[test]
fn harness_is_set_to_start_at_boot_once_when_it_is_there() {
    // Not there: said, nothing asked, the install still done.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(
        !lamp
            .host
            .calls
            .iter()
            .any(|call| call.starts_with("harness"))
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("Harness is not at"))
    );

    // There: asked once, as lampos in the tasks part's home.
    let mut lamp = Lamp::new(188, &[]);
    let harness = lamp.on(install::HARNESS);
    fs::create_dir_all(harness.parent().unwrap()).unwrap();
    fs::write(&harness, "#!/bin/sh\n").unwrap();
    let done = lamp.install().unwrap();
    assert!(
        done.changed
            .iter()
            .any(|line| line == "Harness starts at boot as lampos")
    );
    let asked = |lamp: &Lamp| {
        lamp.host
            .calls
            .iter()
            .filter(|call| *call == "harness lampos /var/lib/lampos-tasks")
            .count()
    };
    assert_eq!(asked(&lamp), 1);
    // Run again: nothing to do.
    let again = lamp.install().unwrap();
    assert!(!again.changed.iter().any(|line| line.contains("Harness")));
    assert_eq!(asked(&lamp), 1);

    // The reviewer's case on #934: the CLI wrote its unit (and linger was
    // set) and then failed. The next install asks again, and only a success
    // is recorded.
    let mut lamp = Lamp::new(188, &[]);
    let harness = lamp.on(install::HARNESS);
    fs::create_dir_all(harness.parent().unwrap()).unwrap();
    fs::write(&harness, "#!/bin/sh\n").unwrap();
    for (dir, file) in [
        ("var/lib/systemd/linger", "lampos"),
        (
            "var/lib/lampos-tasks/.config/systemd/user",
            "harnessd.service",
        ),
    ] {
        fs::create_dir_all(lamp.root.join(dir)).unwrap();
        fs::write(lamp.root.join(dir).join(file), "").unwrap();
    }
    lamp.host.harness_refuses = true;
    assert!(lamp.install().is_err());
    lamp.host.harness_refuses = false;
    let retried = lamp.install().unwrap();
    assert!(
        retried
            .changed
            .iter()
            .any(|line| line == "Harness starts at boot as lampos")
    );
    assert_eq!(asked(&lamp), 2, "asked again after the failure");
    lamp.install().unwrap();
    assert_eq!(asked(&lamp), 2, "and not after the success");

    // Refused: the install says why and stops there.
    let mut lamp = Lamp::new(188, &[]);
    let harness = lamp.on(install::HARNESS);
    fs::create_dir_all(harness.parent().unwrap()).unwrap();
    fs::write(&harness, "#!/bin/sh\n").unwrap();
    lamp.host.harness_refuses = true;
    let refused = lamp.install().unwrap_err();
    assert!(refused.contains("no user manager"), "{refused}");
}

impl Lamp {
    /// A lamp with Harness at [`install::HARNESS`].
    fn with_harness() -> Self {
        let lamp = Lamp::new(188, &[]);
        let harness = lamp.on(install::HARNESS);
        fs::create_dir_all(harness.parent().unwrap()).unwrap();
        fs::write(&harness, "#!/bin/sh\n").unwrap();
        lamp
    }

    fn harness_off(&mut self) -> Result<Vec<String>, String> {
        let said = &mut self.said;
        install::harness_off(
            &self.root,
            &self.config.records,
            &mut self.host,
            &mut |line: &str| said.push(line.to_owned()),
        )
    }

    /// What install's Harness at boot leaves: the linger, the CLI's unit
    /// and install's record.
    fn harness_left_on(&self) -> [bool; 3] {
        [
            self.on("var/lib/systemd/linger/lampos").exists(),
            self.on("var/lib/lampos-tasks/.config/systemd/user/harnessd.service")
                .exists(),
            self.on("var/lib/lampos-update/harness-at-boot").exists(),
        ]
    }

    fn harness_on(&mut self) -> Result<Vec<String>, String> {
        let said = &mut self.said;
        install::harness_on(
            &self.root,
            &self.config.records,
            &mut self.host,
            &mut |line: &str| said.push(line.to_owned()),
        )
    }

    fn called(&self, call: &str) -> usize {
        self.host.calls.iter().filter(|c| *c == call).count()
    }

    fn harness_offs(&self) -> usize {
        self.host
            .calls
            .iter()
            .filter(|call| *call == "harness-off lampos /var/lib/lampos-tasks")
            .count()
    }
}

/// #811, the first builder's yes on #705 (18 Oct 11:1x): `harness-off`
/// after a first install leaves no `lampos` linger, no Harness unit and no
/// record, and Harness itself in place; run again it does nothing; an
/// install afterwards sets Harness up again.
#[test]
fn harness_off_after_a_first_install_leaves_no_linger() {
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    assert_eq!(lamp.harness_left_on(), [true, true, true]);
    assert_eq!(lamp.harness_off().unwrap(), Vec::<String>::new());
    assert_eq!(lamp.harness_left_on(), [false, false, false]);
    assert_eq!(lamp.harness_offs(), 1);
    assert!(lamp.on(install::HARNESS).exists(), "Harness itself stays");

    // Again: no record, so nothing is asked and nothing changes.
    let before = tree(&lamp.root);
    assert_eq!(lamp.harness_off().unwrap(), Vec::<String>::new());
    assert_eq!(lamp.harness_offs(), 1);
    assert_eq!(tree(&lamp.root), before);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("nothing to undo"))
    );

    // An install afterwards sets it up again, once.
    let done = lamp.install().unwrap();
    assert!(
        done.changed
            .iter()
            .any(|line| line == "Harness starts at boot as lampos")
    );
    assert_eq!(lamp.harness_left_on(), [true, true, true]);
}

/// The steps, as the real host runs them: `id -u`, then as `lampos` with
/// its `HOME` and runtime folder `harness service uninstall`, `harness
/// stop`, `harness service status --json`, then `loginctl disable-linger`
/// (after the status, which needs the user's manager), then `pgrep`.
#[test]
fn harness_off_runs_uninstall_stop_status_then_the_linger_then_pgrep() {
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    lamp.harness_off().unwrap();
    let harness = "runuser -u lampos -- env HOME=/var/lib/lampos-tasks XDG_RUNTIME_DIR=/run/user/999 /usr/local/bin/harness";
    assert_eq!(
        lamp.host.steps,
        [
            "id -u lampos".to_owned(),
            format!("{harness} service uninstall"),
            format!("{harness} stop"),
            format!("{harness} service status --json"),
            "loginctl disable-linger lampos".to_owned(),
            "rm -f /etc/systemd/system/user@999.service.d/lamp-home.conf".to_owned(),
            "systemctl daemon-reload".to_owned(),
            "pgrep -u lampos -f harnessd".to_owned(),
        ]
    );
}

/// Each step that fails stops there and keeps the record (and the linger
/// with it, install's to undo first), so a later run tries again and
/// finishes. A status that does not answer is said, not a failure.
#[test]
fn a_failing_step_keeps_the_record_and_a_later_run_finishes() {
    for step in [
        "id -u",
        "service uninstall",
        "stop",
        "disable-linger",
        "lamp-home.conf",
    ] {
        let mut lamp = Lamp::with_harness();
        lamp.install().unwrap();
        lamp.host.failing_step = Some(step.into());
        let refused = lamp.harness_off().unwrap_err();
        assert!(refused.contains("refused"), "{step}: {refused}");
        assert_eq!(lamp.harness_left_on(), [true, true, true], "{step}");
        let ran = lamp.host.steps.len();
        assert!(
            lamp.host.steps.last().unwrap().contains(step),
            "{step}: stopped at it, not after: {:?}",
            lamp.host.steps
        );
        lamp.host.failing_step = None;
        assert_eq!(lamp.harness_off().unwrap(), Vec::<String>::new(), "{step}");
        assert_eq!(lamp.harness_left_on(), [false, false, false], "{step}");
        assert_eq!(lamp.host.steps.len(), ran + 8, "{step}: all eight again");
    }
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    lamp.host.failing_step = Some("status --json".into());
    assert_eq!(
        lamp.harness_off().unwrap(),
        ["`harness service status --json` did not answer"]
    );
    assert_eq!(lamp.harness_left_on(), [false, false, false]);
}

/// What Harness still has afterwards, its service in the status or a
/// `harnessd` `pgrep` finds, is said and returned, never stopped, and the
/// record goes all the same: install's part is undone. Without Harness on
/// the lamp nothing can be undone, and the record is kept.
#[test]
fn what_harness_still_has_is_said_and_without_harness_the_record_is_kept() {
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    lamp.host.status_json = STATUS_GONE
        .replace(r#""registered":false"#, r#""registered":true"#)
        .replace(r#""running":false"#, r#""running":true"#)
        .replace(r#""pid":null"#, r#""pid":4242"#);
    lamp.host.pids = "4242\n4243\n".into();
    let left = vec![
        "Harness still has its service: installed false, registered true, running true, pid 4242"
            .to_owned(),
        "harnessd still runs as lampos: pid 4242, 4243".to_owned(),
    ];
    assert_eq!(lamp.harness_off().unwrap(), left);
    assert!(left.iter().all(|line| lamp.said.contains(line)));
    assert!(!lamp.on("var/lib/lampos-update/harness-at-boot").exists());
    assert!(
        !lamp.host.steps.iter().any(|step| step.contains("kill")),
        "never stopped here"
    );

    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    fs::remove_file(lamp.on(install::HARNESS)).unwrap();
    let missing = lamp.harness_off().unwrap_err();
    assert!(missing.contains("Harness is not at"), "{missing}");
    assert_eq!(lamp.harness_offs(), 0);
    assert!(lamp.on("var/lib/lampos-update/harness-at-boot").exists());
}

/// `harness service status --json` as Harness 0.4.17 writes it
/// (`cli/src/lib/serviceCommand.ts`): nothing left once it is neither
/// installed, registered nor running.
#[test]
fn what_harness_service_status_still_has_is_read_from_its_json() {
    assert!(install::service_left(STATUS_GONE).is_empty());
    let running = STATUS_GONE
        .replace(r#""registered":false"#, r#""registered":true"#)
        .replace(r#""running":false"#, r#""running":true"#)
        .replace(r#""pid":null"#, r#""pid":4242"#);
    assert_eq!(
        install::service_left(&running),
        ["Harness still has its service: installed false, registered true, running true, pid 4242"]
    );
    assert_eq!(install::service_left("nonsense").len(), 1);
}

/// A lamp root holding V1's config with this text, if any.
fn v1_lamp(config: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("root/config")).unwrap();
    if let Some(text) = config {
        fs::write(dir.path().join("root/config/config.json"), text).unwrap();
    }
    dir
}

const DEVICE_KEY: &str = "lamp_SECRET-device-key-0123456789";

fn v1_config() -> String {
    format!(r#"{{"llm_api_key":"{DEVICE_KEY}"}}"#)
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// `lamp-keeper prepare`'s call, with what it says.
fn prepare(root: &Path) -> (Result<Vec<&'static str>, String>, Vec<String>) {
    let mut said = Vec::new();
    let result = install::provision_device_key(root, install::RECORDS, &mut |line: &str| {
        said.push(line.to_owned())
    });
    (result, said)
}

#[test]
fn the_device_key_is_provisioned_private_and_only_where_missing() {
    let lamp = v1_lamp(Some(&v1_config()));
    let root = lamp.path();
    let credential = root.join("etc/lampOS/voice/credential");
    fs::create_dir_all(credential.parent().unwrap()).unwrap();
    fs::write(&credential, "the lamp's own").unwrap();
    let (wrote, said) = prepare(root);
    assert_eq!(wrote.unwrap(), [install::JEV_KEY_PATH]);
    assert_eq!(fs::read_to_string(&credential).unwrap(), "the lamp's own");
    let jev = root.join("etc/lampOS/policy/jev.key");
    assert_eq!(fs::read_to_string(&jev).unwrap(), DEVICE_KEY);
    assert_eq!(mode_of(&jev), 0o600);
    // Recorded as install records what it wrote.
    let state = Records::new(root.join("var/lib/lampos-update")).peek_state();
    assert_eq!(
        state.settings.get(install::JEV_KEY_PATH),
        Some(&sha256_hex(DEVICE_KEY.as_bytes()))
    );
    assert!(!state.settings.contains_key(install::CREDENTIAL_PATH));
    // Nothing left beside; run again, nothing written.
    assert_eq!(fs::read_dir(jev.parent().unwrap()).unwrap().count(), 1);
    assert!(prepare(root).0.unwrap().is_empty());
    assert!(said.iter().all(|line| !line.contains(&DEVICE_KEY[..6])));
}

#[test]
fn a_file_left_beside_by_a_run_cut_off_does_not_lend_its_mode_to_the_key() {
    let lamp = v1_lamp(Some(&v1_config()));
    let voice = lamp.path().join("etc/lampOS/voice");
    fs::create_dir_all(&voice).unwrap();
    let beside = voice.join(".credential.lamp-update");
    fs::write(&beside, "half").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o644)).unwrap();
    }
    prepare(lamp.path()).0.unwrap();
    assert_eq!(mode_of(&voice.join("credential")), 0o600);
    assert!(!beside.exists());
}

#[test]
fn without_a_usable_key_nothing_is_written_and_the_key_is_never_said() {
    for config in [
        None,
        Some("not json"),
        Some("{}"),
        Some(r#"{"llm_api_key":""}"#),
        Some("{\"llm_api_key\":\"two\\nlines\"}"),
    ] {
        let lamp = v1_lamp(config);
        // Said and gone on, as install does: setup takes the user's key.
        let (wrote, said) = prepare(lamp.path());
        assert!(wrote.unwrap().is_empty(), "{config:?}");
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said.iter().all(|line| line.starts_with("no key")),
            "{said:?}"
        );
        assert!(!lamp.path().join("etc").exists(), "{config:?}");
    }
    // With both in place no config is needed, and none is read.
    let lamp = v1_lamp(None);
    for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
        let at = lamp.path().join(path.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, "kept").unwrap();
    }
    assert!(prepare(lamp.path()).0.unwrap().is_empty());
    // What is returned and said names paths only.
    let lamp = v1_lamp(Some(&v1_config()));
    let (wrote, said) = prepare(lamp.path());
    let text = format!("{:?} {said:?}", wrote.unwrap());
    assert!(
        !text.contains(&DEVICE_KEY[..6]) && !text.contains(&DEVICE_KEY.len().to_string()),
        "{text}"
    );
}

/// The owner's decision on #806: the key is the user's and a reset wipes it.
/// Prepare follows install's rule (#1102): a key written once, by install or
/// by prepare, and now gone is never put back from V1's config, which may
/// still hold the previous owner's.
#[test]
fn a_reset_then_prepare_does_not_restore_the_old_key() {
    let cleared = |root: &Path| {
        for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
            fs::remove_file(root.join(path.trim_start_matches('/'))).unwrap();
        }
    };
    let gone = |root: &Path| {
        [install::CREDENTIAL_PATH, install::JEV_KEY_PATH]
            .iter()
            .all(|path| !root.join(path.trim_start_matches('/')).exists())
    };
    // Written by prepare, then reset: prepare again writes nothing.
    let lamp = v1_lamp(Some(&v1_config()));
    assert_eq!(prepare(lamp.path()).0.unwrap().len(), 2);
    cleared(lamp.path());
    let (wrote, said) = prepare(lamp.path());
    assert!(wrote.unwrap().is_empty());
    assert!(gone(lamp.path()));
    assert!(
        said.iter()
            .any(|line| line.contains("V1's key is not put back")),
        "{said:?}"
    );

    // Written by install, then reset: prepare writes nothing either.
    let mut lamp = Lamp::new(190, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(lamp.on(install::V1_CONFIG), v1_config()).unwrap();
    lamp.install().unwrap();
    cleared(&lamp.root);
    assert!(prepare(&lamp.root).0.unwrap().is_empty());
    assert!(gone(&lamp.root));
}

/// The follow-up to #1184: prepare reads the records as install does. A
/// corrupt file is set aside and said, and a key a reset removed is then not
/// taken for one never written; one that cannot be read is said and left
/// alone, with nothing written and no refusal (prepare runs before every
/// update).
#[test]
fn prepare_sets_corrupt_records_aside_and_leaves_unreadable_ones_alone() {
    let key_of = |root: &Path, path: &str| root.join(path.trim_start_matches('/'));
    let cleared = |root: &Path| {
        for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
            fs::remove_file(key_of(root, path)).unwrap();
        }
    };
    // Written by prepare, then a reset, and the records damaged.
    let lamp = v1_lamp(Some(&v1_config()));
    assert_eq!(prepare(lamp.path()).0.unwrap().len(), 2);
    cleared(lamp.path());
    let state = lamp.path().join("var/lib/lampos-update/state.json");
    fs::write(&state, b"{ not json").unwrap();
    let (wrote, said) = prepare(lamp.path());
    assert!(wrote.unwrap().is_empty());
    for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
        assert!(!key_of(lamp.path(), path).exists(), "{path}");
    }
    let aside = fs::read_dir(state.parent().unwrap())
        .unwrap()
        .filter(|entry| {
            let name = entry.as_ref().unwrap().file_name();
            name.to_string_lossy().starts_with("state.json.corrupt-")
        })
        .count();
    assert_eq!(aside, 1, "the damaged file is kept beside");
    assert!(
        said.iter()
            .any(|line| line.contains("could not be read") && line.contains("begun again")),
        "{said:?}"
    );
    assert_eq!(
        said.iter()
            .filter(|line| line.contains("the records were set aside: V1's key is not copied"))
            .count(),
        2,
        "{said:?}"
    );
    assert!(said.iter().all(|line| !line.contains(&DEVICE_KEY[..6])));
    // What the set-aside means lasts (Third on #1263): the next prepare
    // finds the keys recorded as once written and still puts nothing back.
    for run in 0..2 {
        let (wrote, said) = prepare(lamp.path());
        assert!(wrote.unwrap().is_empty(), "run {run} wrote V1's key back");
        for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
            assert!(!key_of(lamp.path(), path).exists(), "{path}");
        }
        assert!(
            said.iter()
                .any(|line| line.contains("V1's key is not put back")),
            "{said:?}"
        );
    }

    // Records that cannot be read: said, nothing written, nothing refused.
    let lamp = v1_lamp(Some(&v1_config()));
    let state = lamp.path().join("var/lib/lampos-update/state.json");
    fs::create_dir_all(&state).unwrap();
    let (wrote, said) = prepare(lamp.path());
    assert!(wrote.unwrap().is_empty());
    assert!(state.is_dir(), "never written over");
    assert!(!lamp.path().join("etc").exists());
    assert!(
        said.iter()
            .any(|line| line.starts_with("the records cannot be read")),
        "{said:?}"
    );
    // Readable again: the key is provisioned as before.
    fs::remove_dir(&state).unwrap();
    assert_eq!(prepare(lamp.path()).0.unwrap().len(), 2);
}

/// The reviewer on #1263: the set-aside must hold for keys that are there
/// when the records are lost (a later reset removes them), and on the run
/// after one that could not save its mark (no `state.json`, a
/// `state.json.corrupt-*` beside it).
#[test]
fn a_set_aside_holds_for_keys_there_and_after_a_mark_that_was_not_saved() {
    let keys = [install::CREDENTIAL_PATH, install::JEV_KEY_PATH];
    let at = |root: &Path, path: &str| root.join(path.trim_start_matches('/'));
    let damage = |root: &Path| {
        fs::write(root.join("var/lib/lampos-update/state.json"), b"{ not json").unwrap();
    };
    let reset = |root: &Path| {
        for path in keys {
            fs::remove_file(at(root, path)).unwrap();
        }
    };
    let none = |root: &Path| keys.iter().all(|path| !at(root, path).exists());

    // (a) prepare writes both, the records are damaged with the keys there,
    // prepare, a reset: neither prepare nor install puts V1's key back.
    let lamp = v1_lamp(Some(&v1_config()));
    assert_eq!(prepare(lamp.path()).0.unwrap().len(), 2);
    damage(lamp.path());
    assert!(prepare(lamp.path()).0.unwrap().is_empty());
    reset(lamp.path());
    assert!(prepare(lamp.path()).0.unwrap().is_empty());
    assert!(none(lamp.path()), "prepare after a reset wrote V1's key");

    // (b) install, setup writes the user's key, damage, install, a reset,
    // install: nothing written; then prepare writes nothing either.
    let mut lamp = Lamp::new(195, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(lamp.on(install::V1_CONFIG), v1_config()).unwrap();
    lamp.install().unwrap();
    fs::write(lamp.on(install::CREDENTIAL_PATH), "new-users-key").unwrap();
    damage(&lamp.root);
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(install::CREDENTIAL_PATH)).unwrap(),
        "new-users-key"
    );
    reset(&lamp.root);
    lamp.install().unwrap();
    assert!(none(&lamp.root), "install after a reset wrote V1's key");
    assert!(prepare(&lamp.root).0.unwrap().is_empty());
    assert!(none(&lamp.root));

    // A run that set the records aside and could not save its mark leaves
    // no `state.json` and the damaged file beside: taken as set aside.
    let lamp = v1_lamp(Some(&v1_config()));
    let folder = lamp.path().join("var/lib/lampos-update");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("state.json.corrupt-1"), b"{ not json").unwrap();
    let (wrote, said) = prepare(lamp.path());
    assert!(wrote.unwrap().is_empty());
    assert!(none(lamp.path()));
    assert!(
        said.iter()
            .any(|line| line.contains("the records were set aside")),
        "{said:?}"
    );
    // That run saves the mark, and the next one keeps to it.
    assert!(folder.join("state.json").exists());
    assert!(prepare(lamp.path()).0.unwrap().is_empty());
    assert!(none(lamp.path()));
    // A blank lamp, no records and nothing beside: V1's key, as before.
    let blank = v1_lamp(Some(&v1_config()));
    assert_eq!(prepare(blank.path()).0.unwrap().len(), 2);
}

/// Where prepare's record and install's rule meet: a key prepare wrote and a
/// reset removed is not put back by the install that follows the migration.
#[test]
fn prepare_then_reset_then_install_does_not_restore_the_old_key() {
    let mut lamp = Lamp::new(191, &[]);
    fs::create_dir_all(lamp.on("/root/config")).unwrap();
    fs::write(lamp.on(install::V1_CONFIG), v1_config()).unwrap();
    assert_eq!(prepare(&lamp.root).0.unwrap().len(), 2);
    for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
        fs::remove_file(lamp.on(path)).unwrap();
    }
    lamp.install().unwrap();
    for path in [install::CREDENTIAL_PATH, install::JEV_KEY_PATH] {
        assert!(!lamp.on(path).exists(), "{path}");
        assert!(
            lamp.said
                .iter()
                .any(|line| line.contains(path) && line.contains("V1's key is not put back")),
            "{:?}",
            lamp.said
        );
    }
}

/// #820: a home lamp outside +07 keeps its own clock. Expression, memory and
/// skills follow the system's zone (`--zone system`: `/etc/localtime`, which
/// setup points at the person's zone, across summer time, #1384), and
/// schedule takes `zone.env`, which setup writes again with that zone. None
/// keeps the offset install found in the image.
#[test]
fn the_parts_that_keep_local_time_read_the_zone_setup_saves() {
    let unit = |name: &str| {
        install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    };
    for name in [
        "lamp-expression.service",
        "lamp-memory.service",
        "lamp-skills.service",
    ] {
        let text = unit(name);
        let start = text
            .lines()
            .find(|line| line.starts_with("ExecStart="))
            .unwrap();
        let words: Vec<&str> = start.split_whitespace().collect();
        assert!(
            words.windows(2).any(|pair| pair == ["--zone", "system"]),
            "{name}: {start}"
        );
        assert!(!text.contains("--utc-offset-minutes"), "{name}");
        assert!(
            !text.contains("EnvironmentFile=/etc/lampOS/zone.env"),
            "{name}"
        );
    }
    let schedule = unit("lamp-schedule.service");
    assert!(schedule.contains("EnvironmentFile=/etc/lampOS/zone.env"));
    assert!(schedule.contains(" --zone ${LAMP_ZONE} "));
}

/// #820 (the first builder, 20:46): a lamp set up before setup wrote
/// `zone.env` keeps the image's zone there, which schedule reads, while its
/// `/etc/timezone` names the person's. The next install writes it again from
/// the saved zone; a file that already names that zone is left as it is.
#[test]
fn a_lamp_set_up_before_setup_wrote_zone_env_gets_its_saved_zone_at_the_next_install() {
    let zone_lines = |lamp: &Lamp| -> Vec<String> {
        fs::read_to_string(lamp.on("/etc/lampOS/zone.env"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert_eq!(
        zone_lines(&lamp),
        ["LAMP_ZONE=Asia/Ho_Chi_Minh", "LAMP_UTC_OFFSET_MINUTES=420"]
    );
    // A wrong /etc/timezone, a zone the lamp's zone data lacks, never
    // replaces a good file: schedule would not start on it (the reviewer).
    fs::create_dir_all(lamp.on("/usr/share/zoneinfo/America")).unwrap();
    fs::write(lamp.on("/usr/share/zoneinfo/America/New_York"), b"TZif2").unwrap();
    fs::write(lamp.on("/etc/timezone"), "Mars/Olympus_Mons\n").unwrap();
    lamp.install().unwrap();
    assert_eq!(
        zone_lines(&lamp),
        ["LAMP_ZONE=Asia/Ho_Chi_Minh", "LAMP_UTC_OFFSET_MINUTES=420"]
    );
    assert!(
        lamp.said.iter().any(|line| line.contains(
            "the lamp names Mars/Olympus_Mons as its zone, which is not a zone in /usr/share/zoneinfo"
        )),
        "{:?}",
        lamp.said
    );
    // Set up in New York by the setup of before: /etc/timezone only.
    fs::write(lamp.on("/etc/timezone"), "America/New_York\n").unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        zone_lines(&lamp),
        ["LAMP_ZONE=America/New_York", "LAMP_UTC_OFFSET_MINUTES=0"]
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("follows the zone this lamp was set up in, America/New_York")),
        "{:?}",
        lamp.said
    );
    // Once it names the saved zone, the file is the lamp's: kept as it is.
    let kept = "# by hand\nLAMP_ZONE=America/New_York\nLAMP_UTC_OFFSET_MINUTES=-240\n";
    fs::write(lamp.on("/etc/lampOS/zone.env"), kept).unwrap();
    lamp.said.clear();
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on("/etc/lampOS/zone.env")).unwrap(),
        kept
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains("follows the zone"))
    );
}

/// #830: a product lamp with no link says so once and keeps going. The voice
/// starts without one and has the offline line (`LAMP_START_OFFLINE`,
/// `LAMP_OFFLINE_VOICE`), and health publishes `lamp/online` from its link
/// (`--online on`). The internet probe (`--reach-probe`, `LAMP_REACH_PROBE`)
/// is only for a lamp with the voice's second route: without it, a relay
/// outage would read as online and a person's speech be dropped without a
/// word (docs/offline.md, "The relay is not the internet").
#[test]
fn a_product_lamp_without_a_link_says_so_once_and_never_drops_speech_in_silence() {
    let unit = |name: &str| {
        install::PRODUCT
            .iter()
            .find(|(unit, _)| *unit == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    };
    let settings = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let voice = settings(unit("lamp-voice.service"));
    for line in [
        "Environment=LAMP_START_OFFLINE=on",
        "Environment=LAMP_OFFLINE_VOICE=on",
    ] {
        assert!(
            voice.iter().any(|set| set == line),
            "the voice lacks {line}"
        );
    }
    let health = settings(unit("lamp-health.service"));
    let start = health
        .iter()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    let words: Vec<&str> = start.split_whitespace().collect();
    assert!(
        words.windows(2).any(|pair| pair == ["--online", "on"]),
        "{start}"
    );
    let second_route = voice
        .iter()
        .any(|line| line.contains("LAMP_VOICE_SECOND_CONFIG="));
    for (name, text) in install::PRODUCT {
        let probes = settings(text)
            .iter()
            .any(|line| line.contains("LAMP_REACH_PROBE=") || line.contains("--reach-probe"));
        assert!(
            !probes || second_route,
            "{name} asks the internet with no second route for the voice"
        );
    }
}

/// #1676 B5: `harness-on` on an installed lamp with no Harness runs Harness's
/// own installer as `lampos`, with its `HOME`, its runtime folder, a plain
/// `PATH` and `SHELL=/bin/sh`, checks the launcher, links it at HARNESS, sets
/// Harness at boot with install's record, and starts the tasks part again.
/// Run again, it changes nothing and starts nothing.
#[test]
fn harness_on_installs_with_harnesss_own_installer_then_sets_it_at_boot() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(!lamp.on(install::HARNESS).exists());
    assert_eq!(lamp.harness_left_on(), [false, false, false]);
    let changed = lamp.harness_on().unwrap();
    assert_eq!(
        changed,
        [
            "Harness installed for lampos by its own installer, at /usr/local/bin/harness",
            "Harness starts at boot as lampos",
            "lamp-tasks.service started again",
        ]
    );
    assert_eq!(
        lamp.host.steps,
        [
            "id -u lampos".to_owned(),
            "install -d -o lampos -g lampos -m 0700 /var/lib/lampos-tasks".to_owned(),
            format!(
                "runuser -u lampos -- env HOME=/var/lib/lampos-tasks XDG_RUNTIME_DIR=/run/user/999 PATH=/usr/local/bin:/usr/bin:/bin SHELL=/bin/sh sh -c curl -fsSL --proto =https --tlsv1.2 {} | sh",
                install::HARNESS_INSTALLER
            ),
            "test -x /var/lib/lampos-tasks/.local/bin/harness".to_owned(),
            "ln -sfn /var/lib/lampos-tasks/.local/bin/harness /usr/local/bin/harness".to_owned(),
        ]
    );
    assert!(lamp.on(install::HARNESS).exists());
    assert_eq!(lamp.harness_left_on(), [true, true, true]);
    assert_eq!(lamp.called("restart lamp-tasks.service"), 1);

    let before = tree(&lamp.root);
    assert_eq!(lamp.harness_on().unwrap(), Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);
    assert_eq!(lamp.called("harness-on lampos /var/lib/lampos-tasks"), 1);
    assert_eq!(lamp.called("restart lamp-tasks.service"), 1);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("already at /usr/local/bin/harness"))
    );
}

/// A Harness already at HARNESS (put there by hand, or by an older run) is
/// kept as it is: nothing is installed over it, and only what is missing,
/// Harness at boot, is set.
#[test]
fn harness_on_keeps_a_harness_already_there() {
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    fs::remove_file(lamp.on("var/lib/lampos-update/harness-at-boot")).unwrap();
    let changed = lamp.harness_on().unwrap();
    assert_eq!(
        changed,
        [
            "Harness starts at boot as lampos",
            "lamp-tasks.service started again"
        ]
    );
    assert_eq!(lamp.called("harness-on lampos /var/lib/lampos-tasks"), 0);
    assert!(lamp.host.steps.is_empty());
}

/// A `harness version` that runs past its bound is not known, not missing:
/// the Harness there, ours or not, is kept and never installed over, and
/// one read in time after its installer ran is not said too old.
#[test]
fn harness_on_keeps_a_harness_whose_version_cannot_be_read_in_time() {
    let late = || {
        Err("runuser … harness version: did not end within 30 s: it ran on, or a child it left held its output".to_owned())
    };
    let ours = |lamp: &Lamp| {
        let link = lamp.on(install::HARNESS);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(
            format!("{}/.local/bin/harness", install::HARNESS_HOME),
            &link,
        )
        .unwrap();
    };
    let installs = |lamp: &Lamp| lamp.called("harness-on lampos /var/lib/lampos-tasks");
    // Ours, too slow to say: kept, nothing installed, nothing too old.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    ours(&lamp);
    lamp.host.versions = [late()].into();
    lamp.harness_on().unwrap();
    assert_eq!(installs(&lamp), 0);
    assert!(
        lamp.said.iter().any(|line| line.starts_with(
            "Harness at /usr/local/bin/harness is kept as it is: its version could not be read in time"
        )),
        "{:?}",
        lamp.said
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.starts_with("harness_too_old:"))
    );
    // Not ours, too slow to say: kept, and not said too old.
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    lamp.host.versions = [late()].into();
    lamp.harness_on().unwrap();
    assert_eq!(installs(&lamp), 0);
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.starts_with("harness_too_old:"))
    );
    // Ours and old, its installer ran, then too slow to say: not too old.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    ours(&lamp);
    lamp.host.versions = [Ok("0.4.19\n".into()), late()].into();
    lamp.harness_on().unwrap();
    assert_eq!(installs(&lamp), 1);
    assert!(
        lamp.said.iter().any(|line| line.starts_with(
            "Harness's own installer ran, from 0.4.19; its version could not be read in time"
        )),
        "{:?}",
        lamp.said
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.starts_with("harness_too_old:"))
    );
}

/// #1676, the first builder at 09:46: a Harness `harness-on` put there (the
/// link to the launcher in its HOME) older than 0.4.23 is brought up to the
/// version Harness's own installer ships, with `harness version` read before
/// and after; one new enough is kept; a failed or still too old install
/// keeps it and says `harness_too_old`, and so does an older Harness put
/// there otherwise, which is never installed over.
#[test]
fn harness_on_updates_its_own_older_harness_and_says_one_too_old() {
    let ours = |lamp: &Lamp| {
        let link = lamp.on(install::HARNESS);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(
            format!("{}/.local/bin/harness", install::HARNESS_HOME),
            &link,
        )
        .unwrap();
    };
    let installs = |lamp: &Lamp| lamp.called("harness-on lampos /var/lib/lampos-tasks");
    let too_old = |lamp: &Lamp| {
        lamp.said
            .iter()
            .filter(|line| line.starts_with("harness_too_old:"))
            .cloned()
            .collect::<Vec<_>>()
    };
    // Updated: 0.4.19 before, 0.4.24 after.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    ours(&lamp);
    lamp.host.versions = [Ok("0.4.19\n".into()), Ok("v0.4.24\n".into())].into();
    let changed = lamp.harness_on().unwrap();
    assert_eq!(
        changed,
        [
            "Harness updated from 0.4.19 to 0.4.24 by its own installer, for signing in by a spoken code",
            "Harness starts at boot as lampos",
            "lamp-tasks.service started again",
        ]
    );
    assert_eq!(installs(&lamp), 1);
    assert!(
        lamp.host
            .steps
            .iter()
            .any(|step| step.contains("install.sh | sh"))
    );
    // New enough: kept, nothing installed.
    lamp.host.versions = [Ok("0.4.24\n".into())].into();
    assert_eq!(lamp.harness_on().unwrap(), Vec::<String>::new());
    assert_eq!(installs(&lamp), 1);
    assert!(
        lamp.said.iter().any(
            |line| line == "Harness 0.4.24 is already at /usr/local/bin/harness: kept as it is"
        )
    );
    // Its installer failed: kept, said, and the rest goes on.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    ours(&lamp);
    lamp.host.failing_step = Some("curl".into());
    lamp.host.versions = [Ok("0.4.19\n".into()), Ok("0.4.19\n".into())].into();
    let changed = lamp.harness_on().unwrap();
    assert!(
        !changed.iter().any(|line| line.contains("updated")),
        "{changed:?}"
    );
    let said = too_old(&lamp);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("installer failed") && said[0].contains("Harness 0.4.19 is kept"));
    assert!(
        fs::symlink_metadata(lamp.on(install::HARNESS))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // Installed, and still too old.
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    ours(&lamp);
    lamp.host.versions = [Ok("0.4.19\n".into()), Ok("0.4.20\n".into())].into();
    let changed = lamp.harness_on().unwrap();
    assert!(
        !changed.iter().any(|line| line.contains("updated")),
        "{changed:?}"
    );
    assert!(too_old(&lamp)[0].contains("Harness is 0.4.20 after its own installer"));
    // Put there otherwise, older: never installed over.
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    lamp.host.versions = [Err("harness version: not found".into())].into();
    let changed = lamp.harness_on().unwrap();
    assert!(
        !changed.iter().any(|line| line.contains("updated")),
        "{changed:?}"
    );
    assert_eq!(installs(&lamp), 0);
    assert!(too_old(&lamp)[0].contains("wasn't put there by harness-on"));
    assert!(lamp.host.steps.is_empty());
}

#[test]
fn harness_versions_are_read_as_harness_prints_them() {
    for (text, version) in [
        ("0.4.23\n", Some((0, 4, 23))),
        ("v0.4.24", Some((0, 4, 24))),
        ("harness 0.5.0-beta.1", Some((0, 5, 0))),
        ("0.4", None),
        ("", None),
    ] {
        assert_eq!(install::harness_version_of(text), version, "{text}");
    }
    assert!(install::harness_version_of("0.4.19").unwrap() < install::HARNESS_LEAST);
    assert!(install::harness_version_of("0.10.0").unwrap() > install::HARNESS_LEAST);
}

/// A step that fails stops there: no link to nothing, nothing recorded, the
/// tasks part not started again, and a later run tries again from the start.
/// Without lampOS installed, it refuses before asking anything.
#[test]
fn harness_on_stops_at_a_failing_step_and_leaves_no_link() {
    let mut lamp = Lamp::new(188, &[]);
    assert!(
        lamp.harness_on()
            .unwrap_err()
            .contains("lampOS is not installed")
    );
    assert!(lamp.host.calls.is_empty());
    lamp.install().unwrap();
    for step in ["curl", "test -x"] {
        lamp.host.steps.clear();
        lamp.host.failing_step = Some(step.into());
        let error = lamp.harness_on().unwrap_err();
        assert!(error.contains("refused"), "{error}");
        assert!(!lamp.on(install::HARNESS).exists(), "{step}");
        assert_eq!(lamp.harness_left_on(), [false, false, false], "{step}");
        assert_eq!(lamp.called("restart lamp-tasks.service"), 0);
        assert!(
            !lamp.host.steps.iter().any(|s| s.starts_with("ln ")),
            "{step}"
        );
    }
    lamp.host.failing_step = None;
    lamp.harness_on().unwrap();
    assert!(lamp.on(install::HARNESS).exists());
}

/// #1676, shown on lamp-75c9 on 19 Oct: Harness at boot gives `lampos`'s own
/// user manager, and no part, the `HOME` Harness writes its unit under, so
/// `harness service install` finds it. The steps in order: the drop-in,
/// `daemon-reload` and `try-restart` of that manager before the linger
/// starts it, then the service install. `harness-off` takes the drop-in
/// away after the linger.
#[test]
fn harness_at_boot_gives_lamposs_manager_its_home_first() {
    let mut lamp = Lamp::with_harness();
    lamp.install().unwrap();
    let harness = "runuser -u lampos -- env HOME=/var/lib/lampos-tasks XDG_RUNTIME_DIR=/run/user/999 /usr/local/bin/harness";
    assert_eq!(
        lamp.host.boot_steps,
        [
            "id -u lampos".to_owned(),
            "install -d -o lampos -g lampos -m 0700 /var/lib/lampos-tasks".to_owned(),
            "systemctl daemon-reload".to_owned(),
            "systemctl try-restart user@999.service".to_owned(),
            "loginctl enable-linger lampos".to_owned(),
            format!("{harness} service install"),
        ]
    );
    let drop_in = lamp.on("etc/systemd/system/user@999.service.d/lamp-home.conf");
    assert_eq!(
        fs::read_to_string(&drop_in).unwrap(),
        "[Service]\nEnvironment=HOME=/var/lib/lampos-tasks\n"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&drop_in).unwrap().permissions().mode() & 0o7777,
        0o644
    );
    lamp.harness_off().unwrap();
    assert!(!drop_in.exists(), "harness-off takes it away");
}

/// A refused service install stops there: the drop-in is written (the next
/// run writes it again, the same), nothing is recorded, and a later install
/// asks again.
#[test]
fn a_refused_service_install_records_nothing_and_keeps_the_drop_in() {
    let mut lamp = Lamp::with_harness();
    lamp.host.harness_refuses = true;
    assert!(lamp.install().is_err());
    assert!(!lamp.on("var/lib/lampos-update/harness-at-boot").exists());
    assert!(
        lamp.on("etc/systemd/system/user@999.service.d/lamp-home.conf")
            .exists()
    );
    assert!(
        lamp.host
            .boot_steps
            .last()
            .unwrap()
            .ends_with("service install")
    );
    lamp.host.harness_refuses = false;
    lamp.install().unwrap();
    assert!(lamp.on("var/lib/lampos-update/harness-at-boot").exists());
}

/// The reviewer on #1788: a lamp that has not run `install` since #1680 has
/// no lamp-update-back at its fixed path. Its unit's condition then skips
/// it, from the timer or from lamp-update's `OnFailure=`, rather than
/// failing every 2 min with a missing `ExecStart`.
#[test]
fn lamp_update_back_waits_for_its_binary_rather_than_failing() {
    let back = PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-update-back.service")
        .unwrap()
        .1;
    let unit = section(back, "Unit");
    assert!(
        unit.contains(&format!("ConditionPathExists={}", lamp_update::own::BACK).as_str()),
        "{unit:?}"
    );
    let exec = section(back, "Service")
        .into_iter()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(exec.starts_with(&format!("ExecStart={} ", lamp_update::own::BACK)));
}

const RESET_PLAN: &str = "/etc/lampOS/setup/reset-plan.json";
const PLAN_KEPT: &str = "/var/lib/lampos-update/reset-plan.json.before-188";

fn put_plan(lamp: &Lamp, text: &str) {
    let at = lamp.on(RESET_PLAN);
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, text).unwrap();
}

/// #806, found on lamp-06c1 on 21 Oct: `install --setup on` kept a plan it
/// had no record of as "this lamp's own". The plan is the product's and
/// never the person's: install places the build's, the old one kept in the
/// records first, and records it.
#[test]
fn install_setup_on_replaces_a_hand_placed_plan_and_keeps_the_old_in_the_records() {
    for old in [
        "{\"placed\": \"by hand\"}\n",
        include_str!("fixtures/reset-plan-4de8ebe1c.json"),
    ] {
        let mut lamp = Lamp::new(188, &[]);
        lamp.config.setup = true;
        put_plan(&lamp, old);
        lamp.install().unwrap();
        assert_eq!(
            fs::read_to_string(lamp.on(RESET_PLAN)).unwrap(),
            install::RESET_PLAN
        );
        assert_eq!(fs::read_to_string(lamp.on(PLAN_KEPT)).unwrap(), old);
        assert_eq!(
            lamp.state().settings[RESET_PLAN],
            sha256_hex(install::RESET_PLAN.as_bytes())
        );
        assert!(
            !lamp
                .said
                .iter()
                .any(|line| line.starts_with(&format!("kept {RESET_PLAN}"))),
            "{:?}",
            lamp.said
        );
    }
}

/// The build's plan already there, recorded or not: no change, no backup.
#[test]
fn the_builds_plan_already_there_is_no_change_and_no_backup() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.config.setup = true;
    put_plan(&lamp, install::RESET_PLAN);
    lamp.install().unwrap();
    assert!(!lamp.on(PLAN_KEPT).exists());
    assert_eq!(
        lamp.state().settings[RESET_PLAN],
        sha256_hex(install::RESET_PLAN.as_bytes())
    );
    let before = tree(&lamp.root);
    assert_eq!(lamp.install().unwrap().changed, Vec::<String>::new());
    assert_eq!(tree(&lamp.root), before);
}

/// Without `--setup on`, a lamp with a plan (set up from the app) gets the
/// build's, and a lamp with none (set up by hand, as lamp-4ace) gets none:
/// with setup's marker a plan would arm the 5 s reset.
#[test]
fn without_setup_on_a_plan_there_follows_the_build_and_none_is_never_added() {
    let mut none = Lamp::new(188, &[]);
    none.install().unwrap();
    assert!(!none.on(RESET_PLAN).exists());
    assert!(!none.state().settings.contains_key(RESET_PLAN));

    let mut lamp = Lamp::new(188, &[]);
    put_plan(&lamp, include_str!("fixtures/reset-plan-ef6a2b9a7.json"));
    lamp.install().unwrap();
    assert_eq!(
        fs::read_to_string(lamp.on(RESET_PLAN)).unwrap(),
        install::RESET_PLAN
    );
    assert_eq!(
        fs::read_to_string(lamp.on(PLAN_KEPT)).unwrap(),
        include_str!("fixtures/reset-plan-ef6a2b9a7.json")
    );
}

/// (c) on #1722, 08:24: every plan install has shipped, from git, the two
/// read on lamps among them; the one it ships now first.
#[test]
fn the_plans_install_shipped_are_listed() {
    let now = sha256_hex(install::RESET_PLAN.as_bytes());
    assert_eq!(install::RESET_PLAN_SHIPPED[0], now, "put {now} first");
    for (text, start) in [
        (
            include_str!("fixtures/reset-plan-4de8ebe1c.json"),
            "5dff6f81",
        ),
        (
            include_str!("fixtures/reset-plan-ef6a2b9a7.json"),
            "de872e78cb60",
        ),
    ] {
        let sha = sha256_hex(text.as_bytes());
        assert!(sha.starts_with(start));
        assert!(install::RESET_PLAN_SHIPPED.contains(&sha.as_str()), "{sha}");
    }
    let mut unique = install::RESET_PLAN_SHIPPED.to_vec();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), install::RESET_PLAN_SHIPPED.len());
}

/// #2791's product half: a product lamp's look is the eyes' full look, so
/// the voice's full look and the eyes' `--full-look` go on together, with
/// the same file, and the sight file is encoded from the look's picture.
#[test]
fn the_product_full_look_is_on_in_both_units_with_one_file() {
    let eyes = include_str!("../../../deploy/product/lamp-eyes.service");
    let voice = include_str!("../../../deploy/product/lamp-voice.service");
    let exec = eyes
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(
        exec.contains(
            " --full-look 1280x960 --full-look-file /run/lampos-eyes/look.jpg --sight-from-look on"
        ),
        "{exec}"
    );
    assert!(
        exec.contains(" --sight-file /run/lampos-eyes/sight.jpg "),
        "{exec}"
    );
    assert!(
        voice
            .lines()
            .any(|line| line == "Environment=LAMP_LOOK_FULL=on")
    );
    assert!(
        voice
            .lines()
            .any(|line| line == "Environment=LAMP_EYES_FULL_LOOK_FILE=/run/lampos-eyes/look.jpg")
    );
}

// Independent #2948 install admission controls, appended without changing
// existing test bodies. These inspect installed files, not runtime privacy.
fn install_review_head_drop_in(disables_head: bool) {
    const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
    const DROP_IN: &str = "/etc/systemd/system/lamp-voice.service.d/30-local.conf";
    const VOICE: &str = "/etc/systemd/system/lamp-voice.service";
    const EXPRESSION: &str = "/etc/systemd/system/lamp-expression.service";
    let mut lamp = Lamp::new(188, &[]);
    let mut pins: serde_json::Value =
        serde_json::from_str(SETTINGS.iter().find(|(path, _)| *path == PINS).unwrap().1).unwrap();
    pins["legacy_head"] = true.into();
    pins["review_owned_marker"] = "keep this local value".into();
    let before_pins = serde_json::to_string_pretty(&pins).unwrap();
    let drop_in = if disables_head {
        "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n"
    } else {
        "[Service]\nEnvironment=LOCAL_REVIEW_MARKER=kept\n"
    };
    for path in [PINS, DROP_IN] {
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
    }
    fs::write(lamp.on(PINS), &before_pins).unwrap();
    fs::write(lamp.on(DROP_IN), drop_in).unwrap();
    let before_voice = fs::read(lamp.on(VOICE)).ok();
    let before_expression = fs::read(lamp.on(EXPRESSION)).ok();
    let result = lamp.install();
    let after_pins: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.on(PINS)).unwrap()).unwrap();
    let voice = fs::read_to_string(lamp.on(VOICE)).unwrap_or_default();
    let expression = fs::read_to_string(lamp.on(EXPRESSION)).unwrap_or_default();
    eprintln!(
        "disabling_override={disables_head}, install_ok={}, legacy_head={:?}, base_voice_head={}, base_expression_head={}, drop_in_kept={}",
        result.is_ok(),
        after_pins.get("legacy_head"),
        voice.contains("Environment=LAMP_HEAD_TOUCH=on"),
        expression.contains(" --head-touch"),
        fs::read_to_string(lamp.on(DROP_IN)).unwrap() == drop_in,
    );
    assert_eq!(fs::read_to_string(lamp.on(DROP_IN)).unwrap(), drop_in);
    if disables_head {
        assert!(
            result.is_err(),
            "install admitted head-enabled Expression beside a preserved Voice head-off override"
        );
        assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), before_pins);
        assert_eq!(fs::read(lamp.on(VOICE)).ok(), before_voice);
        assert_eq!(fs::read(lamp.on(EXPRESSION)).ok(), before_expression);
    } else {
        result.unwrap();
        assert_eq!(after_pins.get("legacy_head"), None);
        assert_eq!(after_pins["review_owned_marker"], "keep this local value");
        for (name, installed) in [
            ("lamp-voice.service", voice),
            ("lamp-expression.service", expression),
        ] {
            assert_eq!(
                installed,
                PRODUCT.iter().find(|(unit, _)| *unit == name).unwrap().1
            );
        }
    }
}

#[test]
fn install_review_head_pair_keeps_a_benign_drop_in() {
    install_review_head_drop_in(false);
}

#[test]
fn install_review_head_pair_refuses_a_disabling_drop_in_before_mutation() {
    install_review_head_drop_in(true);
}

#[test]
fn install_review_head_pair_refuses_a_removed_mute_before_mutation() {
    const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
    const DROP_IN: &str = "/etc/systemd/system/lamp-voice.service.d/40-local-command.conf";
    const VOICE: &str = "/etc/systemd/system/lamp-voice.service";
    const EXPRESSION: &str = "/etc/systemd/system/lamp-expression.service";
    let mut lamp = Lamp::new(188, &[]);
    let product_voice = PRODUCT
        .iter()
        .find(|(name, _)| *name == "lamp-voice.service")
        .unwrap()
        .1;
    let exec = product_voice
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    let mute = " --controls-mute /var/lib/lampOS/voice/mute/record.json";
    assert!(exec.contains(mute));
    let command = exec.replace(mute, "");
    assert!(!command.contains("--controls-mute"));
    let drop_in = format!("[Service]\nExecStart=\n{command}\n");
    let mut pins: serde_json::Value =
        serde_json::from_str(SETTINGS.iter().find(|(path, _)| *path == PINS).unwrap().1).unwrap();
    pins["legacy_head"] = true.into();
    pins["review_owned_marker"] = "keep this local value".into();
    let before_pins = serde_json::to_string_pretty(&pins).unwrap();
    for path in [PINS, DROP_IN] {
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
    }
    fs::write(lamp.on(PINS), &before_pins).unwrap();
    fs::write(lamp.on(DROP_IN), &drop_in).unwrap();
    let before_voice = fs::read(lamp.on(VOICE)).ok();
    let before_expression = fs::read(lamp.on(EXPRESSION)).ok();
    let result = lamp.install();
    let after_pins: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.on(PINS)).unwrap()).unwrap();
    let voice = fs::read_to_string(lamp.on(VOICE)).unwrap_or_default();
    let expression = fs::read_to_string(lamp.on(EXPRESSION)).unwrap_or_default();
    eprintln!(
        "removed_mute_override=true, install_ok={}, legacy_head={:?}, base_voice_head={}, base_expression_head={}, drop_in_kept={}, override_has_mute={}",
        result.is_ok(),
        after_pins.get("legacy_head"),
        voice.contains("Environment=LAMP_HEAD_TOUCH=on"),
        expression.contains(" --head-touch"),
        fs::read_to_string(lamp.on(DROP_IN)).unwrap() == drop_in,
        command.contains("--controls-mute"),
    );
    assert_eq!(fs::read_to_string(lamp.on(DROP_IN)).unwrap(), drop_in);
    assert!(
        result.is_err(),
        "install admitted head-enabled Expression beside a preserved Voice command without --controls-mute"
    );
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), before_pins);
    assert_eq!(fs::read(lamp.on(VOICE)).ok(), before_voice);
    assert_eq!(fs::read(lamp.on(EXPRESSION)).ok(), before_expression);
}

#[test]
fn install_review_head_pair_refusal_precedes_state_and_host_mutation() {
    use std::os::unix::fs::symlink;
    for (path, drop_in) in [
        (
            "/etc/systemd/system/lamp-voice.service.d/30-local.conf",
            "[Service]\nEnvironmentFile=/etc/lampOS/voice/local.env\n",
        ),
        (
            "/etc/systemd/system/lamp-.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
        ),
        (
            "/run/systemd/system/lamp-expression.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_BUS_ROOT=/run/isolated\n",
        ),
        (
            "/usr/local/lib/systemd/system/lamp-voice.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
        ),
        (
            "/etc/systemd/system/lamp-body.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
        ),
        (
            "/usr/local/lib/systemd/system/lamp-body.service.d/30-local.conf",
            "[Service]\nEnvironment=LAMP_BUS_ROOT=/run/isolated\n",
        ),
    ] {
        let mut lamp = Lamp::new(188, &[]);
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
        fs::write(lamp.on(path), drop_in).unwrap();
        let records = Records::new(lamp.on(&lamp.config.records));
        drop(records.lock().unwrap());
        fs::write(lamp.on("/var/lib/lampos-update/state.json"), "{not json").unwrap();
        let current = lamp.on("/opt/lampOS/body/current");
        fs::create_dir_all(current.parent().unwrap()).unwrap();
        symlink("b".repeat(64), &current).unwrap();
        let before = tree(&lamp.root);
        let error = lamp.install().unwrap_err();
        assert!(error.contains("local override"), "{error}");
        assert_eq!(tree(&lamp.root), before, "{path}");
        assert!(lamp.host.calls.is_empty(), "{:?}", lamp.host.calls);
    }
}

#[test]
fn install_review_head_pair_retry_keeps_calibration_and_is_idempotent() {
    const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
    const DROP_IN: &str = "/etc/systemd/system/lamp-voice.service.d/30-local.conf";
    let mut lamp = Lamp::new(188, &[]);
    let mut pins: serde_json::Value =
        serde_json::from_str(SETTINGS.iter().find(|(path, _)| *path == PINS).unwrap().1).unwrap();
    pins["legacy_head"] = true.into();
    pins["every_ms"] = 7.into();
    pins["head_rest"] = serde_json::Value::Array(
        pins["head_pads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pad| serde_json::json!({"name": pad["name"], "rest": "low", "reads": "level"}))
            .collect(),
    );
    let before = serde_json::to_string_pretty(&pins).unwrap();
    for path in [PINS, DROP_IN] {
        fs::create_dir_all(lamp.on(path).parent().unwrap()).unwrap();
    }
    fs::write(lamp.on(PINS), &before).unwrap();
    fs::write(
        lamp.on(DROP_IN),
        "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
    )
    .unwrap();
    assert!(lamp.install().is_err());
    assert_eq!(fs::read_to_string(lamp.on(PINS)).unwrap(), before);
    fs::remove_file(lamp.on(DROP_IN)).unwrap();
    lamp.install().unwrap();
    pins.as_object_mut().unwrap().remove("legacy_head");
    let installed_pins: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.on(PINS)).unwrap()).unwrap();
    assert_eq!(installed_pins, pins);
    let installed = tree(&lamp.root);
    let calls = lamp.host.calls.len();
    let again = lamp.install().unwrap();
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    assert_eq!(tree(&lamp.root), installed);
    assert_eq!(lamp.host.calls.len(), calls);
}

#[test]
fn second_review_body_head_off_refuses_before_bundle_mutation() {
    const DROP_IN: &str = "/etc/systemd/system/lamp-body.service.d/30-local.conf";
    const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
    let mut lamp = Lamp::new(188, &[]);
    fs::create_dir_all(lamp.on(DROP_IN).parent().unwrap()).unwrap();
    let drop_in = "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n";
    fs::write(lamp.on(DROP_IN), drop_in).unwrap();
    let result = lamp.install();
    let pins = fs::read_to_string(lamp.on(PINS)).ok();
    let voice =
        fs::read_to_string(lamp.on("/etc/systemd/system/lamp-voice.service")).unwrap_or_default();
    let expression = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-expression.service"))
        .unwrap_or_default();
    eprintln!(
        "install_ok={}, kept_body_off={}, voice_head_on={}, expression_head_on={}, pins_written={}",
        result.is_ok(),
        fs::read_to_string(lamp.on(DROP_IN)).unwrap() == drop_in,
        voice
            .lines()
            .any(|line| line == "Environment=LAMP_HEAD_TOUCH=on"),
        expression.lines().any(|line| line.starts_with("ExecStart=")
            && line.split_whitespace().any(|word| word == "--head-touch")),
        pins.is_some()
    );
    assert!(
        result.is_err(),
        "install admitted the new head bundle beside a preserved Body head-off override"
    );
    assert!(pins.is_none(), "refusal must precede product pins creation");
}

#[test]
fn install_head_body_keeps_factory_calibration() {
    let mut lamp = Lamp::new(188, &[]);
    let path = lamp.on("/etc/lampOS/body/lamp.env");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let calibration = "# Measured on this lamp.\nBODY_POSES=\"--home 1772,2057,2034 --folded-arm 2050,2059 --stand 1772,2057,2034,2657,2602 --gaze 782,557 --pitch-window 1670,1995 --torque on\"\n";
    fs::write(&path, calibration).unwrap();
    lamp.install().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), calibration);
    let body = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-body.service")).unwrap();
    assert_eq!(
        body,
        PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-body.service")
            .unwrap()
            .1
    );
    assert!(body.contains("Environment=LAMP_HEAD_TOUCH=on"));
    assert!(body.contains("EnvironmentFile=/etc/lampOS/body/lamp.env"));
}

#[test]
fn install_head_body_refuses_calibration_environment_overrides_before_mutation() {
    for calibration in [
        "BODY_POSES=\"--home 1772,2057,2034\"\nLAMP_HEAD_TOUCH=off\n",
        "BODY_POSES=\"--home 1772,2057,2034\"\nLAMP_BUS_ROOT=/run/isolated\n",
        "BODY_POSES=\"--home 1772,2057,2034\\\n\"\n",
        "BODY_POSES=\"--home 1772,2057,2034\nLAMP_HEAD_TOUCH=off\"\n",
    ] {
        let mut lamp = Lamp::new(188, &[]);
        let path = lamp.on("/etc/lampOS/body/lamp.env");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, calibration).unwrap();
        let records = Records::new(lamp.on(&lamp.config.records));
        drop(records.lock().unwrap());
        let before = tree(&lamp.root);
        let error = lamp.install().unwrap_err();
        assert!(error.contains("Body calibration"), "{error}");
        assert_eq!(tree(&lamp.root), before);
        assert!(lamp.host.calls.is_empty(), "{:?}", lamp.host.calls);
    }
}

/// lamp-c0a4's base units after an install to stable (#3733): its
/// `lamp-*.service` units as `deploy/product/` was on 19 Oct
/// (4de8ebe1c), from before lamp-cloud was on in the product.
fn units_of_19_oct() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/units-19-oct");
    let mut units: Vec<(String, String)> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&path).unwrap())
        })
        .collect();
    units.sort();
    assert!(
        units.iter().any(|(name, text)| name == "lamp-cloud.service"
            && text.contains("Off unless set: a drop-in turns it on")),
        "the fixture is 19 Oct's"
    );
    units
}

/// A lamp whose `/etc/systemd/system/` holds 19 Oct's units, and no drop-in.
fn a_lamp_of_19_oct(leave_out: &[&str]) -> Lamp {
    let lamp = Lamp::new(188, leave_out);
    let units = lamp.on(install::UNITS);
    fs::create_dir_all(&units).unwrap();
    for (name, text) in units_of_19_oct() {
        fs::write(units.join(name), text).unwrap();
    }
    lamp
}

/// What `systemctl cat` would print for every product unit on the lamp.
fn printed(lamp: &Lamp) -> String {
    PRODUCT
        .iter()
        .filter(|(name, _)| !name.starts_with('/'))
        .filter_map(|(name, _)| {
            let path = format!("{}{name}", install::UNITS);
            let text = fs::read_to_string(lamp.on(&path)).ok()?;
            Some(format!("# {path}\n{text}\n"))
        })
        .collect()
}

/// #3733: today's install over 19 Oct's units writes every product unit as
/// the build has it, lamp-cloud's with `LAMP_CLOUD=on`; check-units names
/// the old lamp-cloud before, and passes the units after.
#[test]
fn an_install_over_19_octs_units_writes_each_as_the_build_has_it() {
    use lamp_update::check_units;
    let mut lamp = a_lamp_of_19_oct(&[]);
    let before = check_units::check(
        &check_units::embedded(),
        &check_units::parse(&printed(&lamp)),
    );
    assert!(!before.passes());
    assert!(matches!(
        before.units["lamp-cloud.service"],
        check_units::Verdict::Differs(_)
    ));
    let said = before.said();
    assert!(
        said.contains("unit /etc/systemd/system/lamp-cloud.service leaves LAMP_CLOUD off; the product sets it on\n"),
        "{said}"
    );

    lamp.install().unwrap();
    let mut services = 0;
    for (name, text) in PRODUCT {
        if !(name.starts_with("lamp-") && name.ends_with(".service")) {
            continue;
        }
        services += 1;
        let path = format!("{}{name}", install::UNITS);
        assert_eq!(fs::read_to_string(lamp.on(&path)).unwrap(), *text, "{name}");
    }
    assert!(services >= units_of_19_oct().len());
    let cloud = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-cloud.service")).unwrap();
    assert!(
        cloud
            .lines()
            .any(|line| line == "Environment=LAMP_CLOUD=on")
    );
    let after = check_units::check(
        &check_units::embedded(),
        &check_units::parse(&printed(&lamp)),
    );
    assert!(
        after
            .units
            .values()
            .all(|verdict| *verdict == check_units::Verdict::Same),
        "{}",
        after.said()
    );
    assert!(after.switches_off.is_empty(), "{:?}", after.switches_off);
}

/// #3733: an install that ends before its units, for each way it can,
/// ends with an error that begins `units not written: ` and the reason,
/// and leaves 19 Oct's units as they were, not one of them half-written.
/// Once the cause is gone, the next install writes them all.
#[test]
fn an_install_that_ends_before_its_units_says_so_and_keeps_the_old_ones() {
    let units = install::UNITS;
    let old_cloud = units_of_19_oct()
        .into_iter()
        .find(|(name, _)| name == "lamp-cloud.service")
        .unwrap()
        .1;
    let check = |lamp: &mut Lamp, before: &BTreeMap<PathBuf, String>, why: &str| {
        let error = lamp.install().unwrap_err();
        let prefix = format!("{}: ", install::UNITS_NOT_WRITTEN);
        assert!(error.starts_with(&prefix) && error.contains(why), "{error}");
        assert_eq!(&tree(&lamp.on(units)), before, "{why}");
        assert_eq!(
            fs::read_to_string(lamp.on("/etc/systemd/system/lamp-cloud.service")).unwrap(),
            old_cloud
        );
    };

    // The records locked by another lamp-update.
    let mut lamp = a_lamp_of_19_oct(&[]);
    let before = tree(&lamp.on(units));
    let held = Records::new(lamp.on(&lamp.config.records)).lock().unwrap();
    check(
        &mut lamp,
        &before,
        "units not written: another lamp-update is running",
    );
    drop(held);
    lamp.install().unwrap();
    assert_ne!(tree(&lamp.on(units)), before);

    // The head reader's preflight refuses a drop-in that turns it off.
    let mut lamp = a_lamp_of_19_oct(&[]);
    let drop_in = "/etc/systemd/system/lamp-body.service.d/30-local.conf";
    fs::create_dir_all(lamp.on(drop_in).parent().unwrap()).unwrap();
    fs::write(
        lamp.on(drop_in),
        "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
    )
    .unwrap();
    let before = tree(&lamp.on(units));
    let error = lamp.install().unwrap_err();
    assert!(
        error.starts_with("units not written: ") && error.contains("local override"),
        "{error}"
    );
    assert_eq!(tree(&lamp.on(units)), before);

    // A build without one of the parts.
    let mut lamp = a_lamp_of_19_oct(&["lamp-eyes"]);
    let before = tree(&lamp.on(units));
    check(&mut lamp, &before, "build 188 carries no lamp-eyes");

    // A channel with no build.
    let mut lamp = a_lamp_of_19_oct(&[]);
    lamp.channel
        .files
        .remove(&format!("{CHANNEL}{}", manifest::ENVELOPE));
    let before = tree(&lamp.on(units));
    check(&mut lamp, &before, "the channel has published no build");

    // A part's file that is not what was signed.
    let mut lamp = a_lamp_of_19_oct(&[]);
    lamp.channel.files.insert(
        format!("{CHANNEL}lamp-body"),
        b"not what was signed\n".to_vec(),
    );
    let before = tree(&lamp.on(units));
    check(&mut lamp, &before, "lamp-body does not match its SHA-256");
}

#[cfg(unix)]
mod head_touch_preflight_files {
    use super::*;
    use std::{
        io::{self, Read},
        os::unix::fs::{FileTypeExt, MetadataExt, symlink},
        process::{Child, Command, ExitStatus, Stdio},
        time::{Duration, Instant},
    };

    const CHILD_ROOT: &str = "LAMP_UPDATE_HEAD_PREFLIGHT_TEST_ROOT";
    const DROP_IN: &str = "/etc/systemd/system/lamp-voice.service.d/30-local.conf";
    const TARGET: &str = "/etc/lampOS/head-preflight-local.conf";
    const BENIGN: &str = "[Service]\nEnvironment=LOCAL_PREFLIGHT_MARKER=kept\n";

    #[derive(Clone, Copy, Debug)]
    enum Case {
        Fifo,
        FifoNoWriter,
        BodyFifo,
        Directory,
        Regular,
        RegularSymlink,
        Missing,
        DanglingSymlink,
        NullMask,
    }

    impl Case {
        fn refuses(self) -> bool {
            matches!(
                self,
                Self::Fifo | Self::FifoNoWriter | Self::BodyFifo | Self::Directory
            )
        }

        fn local_path(self) -> &'static str {
            match self {
                Self::BodyFifo => "/etc/lampOS/body/lamp.env",
                _ => DROP_IN,
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Kept {
        device: u64,
        inode: u64,
        mode: u32,
        bytes: Option<String>,
        target: Option<PathBuf>,
    }

    // Never open a special node while taking the fixture's before/after image.
    fn kept(path: &Path) -> io::Result<Option<Kept>> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(Kept {
            device: meta.dev(),
            inode: meta.ino(),
            mode: meta.mode(),
            bytes: if meta.is_file() {
                Some(sha256_hex(&fs::read(path)?))
            } else {
                None
            },
            target: if meta.file_type().is_symlink() {
                Some(fs::read_link(path)?)
            } else {
                None
            },
        }))
    }

    fn image(root: &Path) -> BTreeMap<PathBuf, Kept> {
        let mut result = BTreeMap::new();
        let mut folders = vec![root.to_owned()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if fs::symlink_metadata(&path).unwrap().is_dir() {
                    folders.push(path.clone());
                }
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    kept(&path).unwrap().unwrap(),
                );
            }
        }
        result
    }

    struct Running(Child);

    impl Running {
        fn reap(&mut self) -> io::Result<ExitStatus> {
            if let Ok(Some(status)) = self.0.try_wait() {
                return Ok(status);
            }
            // Even if the status probe or kill reports an error, attempt wait;
            // the child may have exited between the probe and the signal.
            let _ = self.0.kill();
            self.0.wait()
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            // These direct children create no processes or threads of their
            // own beyond the test harness. Reap even on an unexpected error.
            let _ = self.reap();
        }
    }

    fn make_fifo(path: &Path) -> io::Result<()> {
        // rustix does not expose mkfifoat on Apple. Finish and reap this
        // portable fixture utility before starting the install process.
        let mut child = Running(
            Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("mkfifo fixture exited {status}")))
                };
            }
            if Instant::now() >= deadline {
                child.reap()?;
                return Err(io::Error::other("mkfifo fixture setup deadline"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn exercise(case: Case, work: Option<&Path>) {
        let mut lamp = Lamp::new(188, &[]);
        if let Some(work) = work {
            assert!(case.refuses(), "only refusal cases need a child watchdog");
            assert!(lamp.root.starts_with(work), "parent owns all child scratch");
        }
        let path = lamp.on(case.local_path());
        let target = lamp.on(TARGET);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        match case {
            Case::Fifo | Case::FifoNoWriter | Case::BodyFifo => {
                fs::rename(
                    work.expect("FIFO has an owned child").join("prepared-fifo"),
                    &path,
                )
                .unwrap();
                assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
            }
            Case::Directory => fs::create_dir(&path).unwrap(),
            Case::Regular => fs::write(&path, BENIGN).unwrap(),
            Case::RegularSymlink => {
                fs::write(&target, BENIGN).unwrap();
                symlink(&target, &path).unwrap();
            }
            Case::Missing => {}
            Case::DanglingSymlink => symlink(&target, &path).unwrap(),
            Case::NullMask => {
                assert!(
                    fs::metadata("/dev/null")
                        .unwrap()
                        .file_type()
                        .is_char_device()
                );
                symlink("/dev/null", &path).unwrap();
            }
        }
        let mut pins: serde_json::Value =
            serde_json::from_str(SETTINGS.iter().find(|(p, _)| *p == PINS).unwrap().1).unwrap();
        pins["legacy_head"] = true.into();
        pins["preflight_owned_marker"] = "keep".into();
        fs::create_dir_all(lamp.on(PINS).parent().unwrap()).unwrap();
        fs::write(lamp.on(PINS), serde_json::to_vec(&pins).unwrap()).unwrap();
        // The updater may create its lock before preflight. Precreate it so
        // the refusal image tests all subsequent mutation, including records.
        drop(Records::new(lamp.on(&lamp.config.records)).lock().unwrap());
        if case.refuses() {
            fs::create_dir_all(lamp.on("/etc/systemd/system")).unwrap();
            fs::write(lamp.on("/var/lib/lampos-update/state.json"), "{not json").unwrap();
            let current = lamp.on("/opt/lampOS/body/current");
            fs::create_dir_all(current.parent().unwrap()).unwrap();
            symlink("b".repeat(64), &current).unwrap();
            for name in [
                "lamp-voice.service",
                "lamp-expression.service",
                "lamp-controls.service",
                "lamp-body.service",
            ] {
                fs::write(
                    lamp.on(&format!("/etc/systemd/system/{name}")),
                    format!("old local {name}\n"),
                )
                .unwrap();
            }
        }
        let before = image(&lamp.root);
        let drop_in = kept(&path).unwrap();
        let target_file = kept(&target).unwrap();
        assert!(lamp.host.calls.is_empty());
        // Publish atomically, after all setup and the snapshot, immediately
        // before the real install call. No FIFO has been opened yet.
        if let Some(work) = work {
            fs::write(work.join("ready.tmp"), lamp.root.to_str().unwrap()).unwrap();
            fs::rename(work.join("ready.tmp"), work.join("ready")).unwrap();
        }
        let result = lamp.install();
        if case.refuses() {
            let error = result.unwrap_err();
            assert!(error.contains(case.local_path()), "{error}");
            assert_eq!(image(&lamp.root), before, "{case:?}: refusal mutated files");
            assert!(lamp.host.calls.is_empty(), "{:?}", lamp.host.calls);
            assert!(lamp.host.steps.is_empty() && lamp.host.boot_steps.is_empty());
        } else {
            assert_eq!(result.unwrap().sequence, 188);
            assert_eq!(kept(&path).unwrap(), drop_in, "{case:?}: kept override");
            assert_eq!(kept(&target).unwrap(), target_file, "{case:?}: kept target");
            let after: serde_json::Value =
                serde_json::from_slice(&fs::read(lamp.on(PINS)).unwrap()).unwrap();
            pins.as_object_mut().unwrap().remove("legacy_head");
            assert_eq!(after, pins, "normal head-pins migration still happens");
            assert!(!lamp.on("/etc/lampOS/body/lamp.env").exists());
            assert!(
                lamp.host
                    .calls
                    .iter()
                    .any(|call| call.starts_with("enable "))
            );
        }
        if let Some(work) = work {
            fs::write(work.join("finished"), format!("{case:?}")).unwrap();
        }
    }

    fn ready_root(work: &Path) -> io::Result<Option<PathBuf>> {
        let root = match fs::read_to_string(work.join("ready")) {
            Ok(root) => PathBuf::from(root),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !root.starts_with(work) {
            return Err(io::Error::other("ready witness is outside parent scratch"));
        }
        Ok(Some(root))
    }

    fn parent(case: Case, name: &str, work: &Path) -> io::Result<()> {
        if matches!(case, Case::Fifo | Case::FifoNoWriter | Case::BodyFifo) {
            make_fifo(&work.join("prepared-fifo"))?;
        }
        let log = work.join("child.log");
        let output = fs::File::create(&log)?;
        let mut child = Running(
            Command::new(std::env::current_exe()?)
                .args(["--exact", name, "--nocapture", "--test-threads=1"])
                .env(CHILD_ROOT, work)
                .env("TMPDIR", work)
                .stdin(Stdio::null())
                .stdout(Stdio::from(output.try_clone()?))
                .stderr(Stdio::from(output))
                .spawn()?,
        );
        let setup_deadline = Instant::now() + Duration::from_secs(15);
        let mut install_deadline = None;
        let mut fifo = None;
        let mut writer: Option<fs::File> = None;
        let status = loop {
            if install_deadline.is_none()
                && let Some(root) = ready_root(work)?
            {
                fifo = Some(root.join(case.local_path().trim_start_matches('/')));
                install_deadline = Some(Instant::now() + Duration::from_secs(5));
            }
            if let Some(status) = child.0.try_wait()? {
                break Some(status);
            }
            if matches!(case, Case::Fifo | Case::BodyFifo)
                && writer.is_none()
                && let Some(path) = &fifo
            {
                match rustix::fs::open(
                    path,
                    rustix::fs::OFlags::WRONLY
                        | rustix::fs::OFlags::NONBLOCK
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                ) {
                    Ok(fd) => {
                        let file: fs::File = fd.into();
                        if !file.metadata()?.file_type().is_fifo() {
                            return Err(io::Error::other("FIFO witness changed file type"));
                        }
                        // A nonblocking FIFO writer opens only once a reader
                        // has arrived. Hold it without data: old read_to_string
                        // is now blocked in its read, not fixture preparation.
                        writer = Some(file);
                    }
                    Err(rustix::io::Errno::NXIO | rustix::io::Errno::NOENT) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if Instant::now() >= install_deadline.unwrap_or(setup_deadline) {
                break None;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let fifo_reader_reached = writer.is_some();
        let reaped = child.reap()?;
        drop(writer);
        // A fast child can publish ready and exit between the witness read
        // and try_wait. Consume its final witness after a witnessed exit.
        // A setup timeout still cannot be relabelled as a post-ready timeout.
        let ready = install_deadline.is_some() || (status.is_some() && ready_root(work)?.is_some());
        let mut journal = String::new();
        fs::File::open(&log)?
            .take(4096)
            .read_to_string(&mut journal)?;
        if !ready {
            return Err(io::Error::other(format!(
                "{case:?}: fixture never reached its ready witness; reaped={reaped}; {journal}"
            )));
        }
        if status.is_none() {
            return Err(io::Error::other(format!(
                "{case:?}: install exceeded its post-ready deadline; FIFO-reader-reached={fifo_reader_reached}; reaped={reaped}; {journal}"
            )));
        }
        if !reaped.success() || !work.join("finished").is_file() {
            return Err(io::Error::other(format!(
                "{case:?}: install child failed after ready; FIFO-reader-reached={fifo_reader_reached}; reaped={reaped}; {journal}"
            )));
        }
        Ok(())
    }

    fn run(case: Case, name: &str) {
        if !case.refuses() {
            // These regular/missing paths cannot block on special-file I/O.
            // Keep the full fresh install and its durability under the suite
            // bound, like the other install tests, rather than a liveness timer.
            exercise(case, None);
            return;
        }
        if let Some(work) = std::env::var_os(CHILD_ROOT) {
            exercise(case, Some(Path::new(&work)));
            return;
        }
        let work = tempfile::tempdir().unwrap();
        let result = parent(case, name, work.path());
        // parent has killed/reaped its child before scratch removal or an
        // assertion panic; no blocked thread or orphaned fixture is left.
        work.close().unwrap();
        result.unwrap();
    }

    #[test]
    fn fifo_drop_in_is_refused_before_mutation() {
        run(
            Case::Fifo,
            "head_touch_preflight_files::fifo_drop_in_is_refused_before_mutation",
        );
    }

    #[test]
    fn fifo_without_a_writer_is_refused_before_mutation() {
        // The separate original-code witness supplies a writer. This case
        // must refuse with no writer at all, including during open itself.
        run(
            Case::FifoNoWriter,
            "head_touch_preflight_files::fifo_without_a_writer_is_refused_before_mutation",
        );
    }

    #[test]
    fn fifo_body_calibration_is_refused_before_mutation() {
        run(
            Case::BodyFifo,
            "head_touch_preflight_files::fifo_body_calibration_is_refused_before_mutation",
        );
    }

    #[test]
    fn directory_drop_in_is_refused_before_mutation() {
        run(
            Case::Directory,
            "head_touch_preflight_files::directory_drop_in_is_refused_before_mutation",
        );
    }

    #[test]
    fn regular_benign_drop_in_is_preserved() {
        run(
            Case::Regular,
            "head_touch_preflight_files::regular_benign_drop_in_is_preserved",
        );
    }

    #[test]
    fn symlink_to_regular_drop_in_is_preserved() {
        run(
            Case::RegularSymlink,
            "head_touch_preflight_files::symlink_to_regular_drop_in_is_preserved",
        );
    }

    #[test]
    fn missing_local_files_keep_install_compatibility() {
        run(
            Case::Missing,
            "head_touch_preflight_files::missing_local_files_keep_install_compatibility",
        );
    }

    #[test]
    fn dangling_drop_in_keeps_missing_file_compatibility() {
        run(
            Case::DanglingSymlink,
            "head_touch_preflight_files::dangling_drop_in_keeps_missing_file_compatibility",
        );
    }

    #[test]
    fn dev_null_drop_in_mask_is_preserved() {
        run(
            Case::NullMask,
            "head_touch_preflight_files::dev_null_drop_in_mask_is_preserved",
        );
    }
}

/// Every shipped unit that hides the body's servo port from itself says it
/// with `-`, so a port not there when the unit starts (a boot race, the body
/// unplugged, a lamp without one) is passed over, and then not hidden for
/// that run: systemd sets up the namespace once, at start, and the next start
/// hides it. Without it systemd fails
/// the unit at start (226/NAMESPACE), and the voice's restart guard left
/// lamp-a0ae mute on 28 Oct. Read from every unit under deploy/, so a new
/// per-lamp copy is held to it too.
#[test]
fn a_servo_port_not_there_never_fails_a_unit_that_hides_it() {
    fn units(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                units(&path, found);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "service")
            {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    units(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy"),
        &mut found,
    );
    let mut hiding = Vec::new();
    for unit in &found {
        for line in fs::read_to_string(unit).unwrap().lines() {
            let Some(paths) = line.strip_prefix("InaccessiblePaths=") else {
                continue;
            };
            for path in paths.split_whitespace() {
                if path.contains("ttyACM") || path.contains("@SERVO_DEVICE@") {
                    assert!(
                        path.starts_with('-'),
                        "{}: InaccessiblePaths={path} fails the unit when the port isn't there; write -{path}",
                        unit.display()
                    );
                    hiding.push(unit.file_name().unwrap().to_owned());
                }
            }
        }
    }
    // The product's, the generic package's, the two per-lamp copies and the
    // attended pilot's: a check that found none would pass on nothing.
    assert!(hiding.len() >= 5, "only {hiding:?} hide the servo port");
}
