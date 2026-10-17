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
}

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
        if self.harness_refuses {
            return Err("harness service install: no user manager".into());
        }
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

    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String> {
        self.calls.push(format!("offset {zone}"));
        Ok(if zone == "Asia/Ho_Chi_Minh" { 420 } else { 0 })
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

/// Every binary the product starts, and the voice's worker: a line of text each.
fn binaries() -> Vec<String> {
    let mut names: Vec<String> = install::parts()
        .into_iter()
        .map(|part| part.binary)
        .collect();
    names.push("lamp-turn-end".into());
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
        for name in binaries() {
            if leave_out.contains(&name.as_str()) {
                continue;
            }
            let bytes = format!("{name} of build {sequence}\n").into_bytes();
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

    for (name, text) in PRODUCT {
        let written = fs::read_to_string(lamp.on(&format!("/etc/systemd/system/{name}"))).unwrap();
        assert_eq!(&written, text, "{name}");
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
    assert_eq!(gated.len(), 19, "{gated:?}");
    for outside in ["lamp-update.service", "lamp-records.timer"] {
        assert!(!gated.contains(&outside), "{outside}");
    }
    let page = include_str!("../../../docs/install.md");
    assert!(page.contains(&format!("`{condition}`")));
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

/// #811, from lamp-06c1: a home lamp gets `lamp-skills` and its first skill,
/// Welcome back, and the voice speaks it through its say service with its
/// own hello off, so the lamp greets once.
#[test]
fn a_blank_lamp_gets_lamp_skills_with_welcome_back_and_greets_once() {
    let mut lamp = Lamp::new(188, &[]);
    lamp.install().unwrap();
    assert!(lamp.on("/opt/lampOS/skills/current/lamp-skills").exists());
    for file in ["skill.toml", "SKILL.md", "lights/warm_glow.toml"] {
        let on_lamp = fs::read(lamp.on(&format!("/var/lib/lampOS/skills/welcome-back/{file}")));
        let ours = fs::read(format!(
            "{}/../../skills/welcome-back/{file}",
            env!("CARGO_MANIFEST_DIR")
        ));
        assert_eq!(on_lamp.unwrap(), ours.unwrap(), "{file}");
    }
    assert!(
        lamp.host
            .calls
            .iter()
            .any(|call| call.contains("lamp-skills.service"))
    );
    let voice = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-voice.service")).unwrap();
    let service = section(&voice, "Service");
    assert!(service.contains(&"Environment=LAMP_SAY=on"));
    assert!(service.contains(&format!("Environment=LAMP_SAY_GROUP={}", install::USER).as_str()));
    assert!(
        !service
            .iter()
            .any(|line| line.starts_with("Environment=LAMP_SPEAK_FIRST="))
    );
    let skills = fs::read_to_string(lamp.on("/etc/systemd/system/lamp-skills.service")).unwrap();
    assert!(skills.contains("--say-socket /run/lampos-say/say.sock"));
    assert!(skills.contains("--dir /var/lib/lampOS/skills"));
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
    let models = install::models().unwrap();
    assert_eq!(models.len(), 7);
    let release = "https://github.com/autonomous-ai/lampOS/releases/download/models-2026-10-16/";
    for model in &models {
        assert!(model.path.starts_with("/opt/lampOS/"), "{}", model.path);
        assert!(model.url.starts_with(release), "{}", model.url);
        assert_eq!(model.sha256.len(), 64);
        assert!(model.bytes > 0);
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
}

const PINS: &str = "/etc/lampOS/controls/controls-pins.json";

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
        .replace("\"VN\"", "\"SG\"");
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
