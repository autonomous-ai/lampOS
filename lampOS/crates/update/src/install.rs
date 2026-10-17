//! `install`: lampOS on a blank lamp, from the newest signed build of its
//! channel (docs/install.md, task #811).
//!
//! On a lamp with the base image and nothing of lampOS, it installs every
//! part of the product (`deploy/product/`, carried in this program): the
//! build's binary of each, in `/opt/lampOS/<part>/<sha256>/` with the link
//! `current` to it; the product units, the target, the slices and the
//! timer; the tmpfiles of the bus and the locks; the parts' users; and the
//! lamp's zone. It enables the lamp and starts nothing: the lamp starts
//! once setup has written its marker (#806).
//!
//! Run again, it changes nothing that is as it should be, and says so.
//! It never writes what is one lamp's own (`/etc/lampOS/body/lamp.env`) or
//! what setup writes, and never a unit's drop-ins. A product setting
//! changed on the lamp since an install wrote it is the lamp's own too, and
//! is kept ([`settings`]).
use crate::{
    fetch::{Channel, Fetch},
    manifest::{self, Manifest},
    records::{Build, Records, State, replace_link, sync_parent, write_whole},
    round::{Hashing, file_sha256, filling},
    unit,
};
use ring::digest;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

/// Every unit of the product, by name, as `deploy/product/` has it at the
/// commit this program was built from.
pub const PRODUCT: &[(&str, &str)] = &[
    (
        "lampos.target",
        include_str!("../../../deploy/product/lampos.target"),
    ),
    (
        "lampos-voice.slice",
        include_str!("../../../deploy/product/lampos-voice.slice"),
    ),
    (
        "lampos-parts.slice",
        include_str!("../../../deploy/product/lampos-parts.slice"),
    ),
    (
        "lamp-voice.service",
        include_str!("../../../deploy/product/lamp-voice.service"),
    ),
    (
        "lamp-body.service",
        include_str!("../../../deploy/product/lamp-body.service"),
    ),
    (
        "lamp-eyes.service",
        include_str!("../../../deploy/product/lamp-eyes.service"),
    ),
    (
        "lamp-name.service",
        include_str!("../../../deploy/product/lamp-name.service"),
    ),
    (
        "lamp-expression.service",
        include_str!("../../../deploy/product/lamp-expression.service"),
    ),
    (
        "lamp-actions.service",
        include_str!("../../../deploy/product/lamp-actions.service"),
    ),
    (
        "lamp-memory.service",
        include_str!("../../../deploy/product/lamp-memory.service"),
    ),
    (
        "lamp-policy.service",
        include_str!("../../../deploy/product/lamp-policy.service"),
    ),
    (
        "lamp-health.service",
        include_str!("../../../deploy/product/lamp-health.service"),
    ),
    (
        "lamp-controls.service",
        include_str!("../../../deploy/product/lamp-controls.service"),
    ),
    (
        "lamp-ears.service",
        include_str!("../../../deploy/product/lamp-ears.service"),
    ),
    (
        "lamp-schedule.service",
        include_str!("../../../deploy/product/lamp-schedule.service"),
    ),
    (
        "lamp-air.service",
        include_str!("../../../deploy/product/lamp-air.service"),
    ),
    // The air's bus at 100 kHz, set as root before the air and the controls
    // start; the same binary as the air's.
    (
        "lamp-air-clock.service",
        include_str!("../../../deploy/product/lamp-air-clock.service"),
    ),
    (
        "lamp-tasks.service",
        include_str!("../../../deploy/product/lamp-tasks.service"),
    ),
    (
        "lamp-skills.service",
        include_str!("../../../deploy/product/lamp-skills.service"),
    ),
    (
        "lamp-records.service",
        include_str!("../../../deploy/product/lamp-records.service"),
    ),
    (
        "lamp-records.timer",
        include_str!("../../../deploy/product/lamp-records.timer"),
    ),
    (
        "lamp-update.service",
        include_str!("../../../deploy/product/lamp-update.service"),
    ),
    // The passive self-test, five minutes after the lamp starts and every
    // six hours (#812). Its service is started by its timer alone.
    (
        "lamp-selftest.service",
        include_str!("../../../deploy/product/lamp-selftest.service"),
    ),
    (
        "lamp-selftest.timer",
        include_str!("../../../deploy/product/lamp-selftest.timer"),
    ),
    // Setup at first boot, its reset, and the start of the lamp once it is
    // set up (#806, #811).
    (
        "lamp-setup.service",
        include_str!("../../../deploy/product/lamp-setup.service"),
    ),
    (
        "lamp-setup-reset.service",
        include_str!("../../../deploy/product/lamp-setup-reset.service"),
    ),
    (
        "lampos-setup-done.path",
        include_str!("../../../deploy/product/lampos-setup-done.path"),
    ),
];

/// The product's settings that no build carries, and where each goes:
/// in `/etc/lampOS/<part>/`, readable by the parts' user (docs/install.md).
pub const SETTINGS: &[(&str, &str)] = &[
    (
        "/etc/lampOS/expression/expression.json",
        include_str!("../../../deploy/product/settings/expression.json"),
    ),
    (
        "/etc/lampOS/controls/controls.json",
        include_str!("../../../deploy/product/settings/controls.json"),
    ),
    (
        "/etc/lampOS/controls/controls-pins.json",
        include_str!("../../../deploy/product/settings/controls-pins.json"),
    ),
    // Records' bound on the voice's sessions, without which its timer runs
    // nothing and they grow without end (Twelve on lamp-cb94, the first
    // builder on #705, 17 Oct): 2.0 GB, as on lamp-4ace, and two weeks.
    (
        "/etc/lampOS/records.json",
        include_str!("../../../deploy/product/settings/records.json"),
    ),
    // The first skill (#811): Welcome back, the hello on arrival. Its tests
    // stay in the repository; `lamp-skills` reloads the folder by itself.
    (
        "/var/lib/lampOS/skills/welcome-back/skill.toml",
        include_str!("../../../skills/welcome-back/skill.toml"),
    ),
    (
        "/var/lib/lampOS/skills/welcome-back/SKILL.md",
        include_str!("../../../skills/welcome-back/SKILL.md"),
    ),
    (
        "/var/lib/lampOS/skills/welcome-back/lights/warm_glow.toml",
        include_str!("../../../skills/welcome-back/lights/warm_glow.toml"),
    ),
];

/// Setup's two settings (docs/setup.md, "On a lamp"), written only with
/// `install --setup on`: with them and no marker, `lamp-setup.service` starts
/// the hotspot at boot. Off unless set, so rerunning install to upgrade the
/// updater on a lamp set up by hand never puts it into setup (#811).
pub const SETUP_SETTINGS: &[(&str, &str)] = &[
    (
        "/etc/lampOS/setup/serve.json",
        include_str!("../../setup/fixtures/serve-own-page.json"),
    ),
    (
        "/etc/lampOS/setup/reset-plan.json",
        include_str!("../../setup/fixtures/reset-plan-product.json"),
    ),
];
/// Models no build carries (#811): where each goes, where it is fetched from,
/// and its size and SHA-256 (`deploy/product/models.json`): one release of
/// ours, with the name part's own `name.onnx`, the one the lamps run.
pub const MODELS: &str = include_str!("../../../deploy/product/models.json");

#[derive(serde::Deserialize)]
struct Models {
    models: Vec<Model>,
}

/// One model of [`MODELS`].
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub path: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
    /// The file is one member of a gzipped tar at `url`, itself pinned.
    #[serde(default)]
    pub archive: Option<Archive>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub bytes: u64,
    pub sha256: String,
    pub member: String,
}

/// The models of [`MODELS`].
pub fn models() -> Result<Vec<Model>, String> {
    serde_json::from_str::<Models>(MODELS)
        .map(|list| list.models)
        .map_err(|error| format!("deploy/product/models.json: {error}"))
}

/// `/etc/tmpfiles.d/lampos.conf`: the bus root and the locks.
pub const TMPFILES: &str = include_str!("../../../deploy/product/lampos.conf");
pub const TMPFILES_PATH: &str = "/etc/tmpfiles.d/lampos.conf";
/// The lamp's zone, for the parts that take it (docs/install.md).
pub const ZONE_PATH: &str = "/etc/lampOS/zone.env";
/// The voice's service: V1's route, the campaign-api proxy with the user's
/// key. Private, as the voice requires (`read_private`: the process user's,
/// 0600), kept up to date while it is install's own.
pub const PROVIDER_PATH: &str = "/etc/lampOS/voice/provider.json";
pub const PROVIDER: &str = include_str!("../../../deploy/product/settings/voice-provider.json");
/// The voice's key, as `provider.json` names it. It is the user's (the owner on
/// #806, 2026-10-16 16:13): setup takes it from the account page or the app,
/// and a reset wipes it. On a lamp that ran V1, install copies V1's
/// `llm_api_key` once, into a lamp that never had one.
pub const CREDENTIAL_PATH: &str = "/etc/lampOS/voice/credential";
/// The Policy's judge key: the same key, for V1's route to Jev
/// (`deploy/product/lamp-policy.service`; the first builder on #811, 13:09:
/// provision it like the voice's).
pub const JEV_KEY_PATH: &str = "/etc/lampOS/policy/jev.key";
/// What each key path is recorded as when the records were set aside: once
/// written, with no hash any file can match. So the rule for a key removed
/// since it was written holds on every later run, not only on the run that
/// found the damage, which saves records with no key in them (Third on
/// #1263). A key setup writes later is kept as the lamp's own.
pub const SET_ASIDE: &str = "unknown: the records were set aside";
/// The key paths [`SET_ASIDE`] marks: the voice's and the Policy's.
pub const KEY_PATHS: [&str; 2] = [CREDENTIAL_PATH, JEV_KEY_PATH];

/// Whether the records in `folder` were set aside: by this run's read
/// (`existed`, and now gone), or by an earlier run that could not save what
/// it marked, which left no `state.json` and a `state.json.corrupt-*` beside
/// it (the reviewer on #1263). Either way, what was written before is not
/// known.
fn records_lost(folder: &Path, existed: bool) -> bool {
    if folder.join("state.json").exists() {
        return false;
    }
    existed
        || fs::read_dir(folder).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("state.json.corrupt-")
            })
        })
}
pub const V1_CONFIG: &str = "/root/config/config.json";
/// Where lamp-update keeps its records on a lamp.
pub const RECORDS: &str = "/var/lib/lampos-update/";
pub const UNITS: &str = "/etc/systemd/system/";

/// The parts' user, and memory's own.
pub const USER: &str = "lampos";
pub const MEMORY_USER: &str = "lampos-memory";
/// The groups `lampos` is put in, each only if the image has it.
pub const GROUPS: &[&str] = &["audio", "video", "dialout", "i2c", "gpio"];

/// The Harness program, as the tasks unit names it, and the home it runs
/// in: the tasks part's state folder (`deploy/product/lamp-tasks.service`).
pub const HARNESS: &str = "/usr/local/bin/harness";
pub const HARNESS_HOME: &str = "/var/lib/lampos-tasks";
/// What says Harness was set to start at boot: written in lamp-update's
/// records only after `harness service install` succeeded. Not the files
/// the CLI leaves: it writes its unit before registering it, and keeps it
/// when a later step fails (the reviewer on #934), so they prove nothing.
const HARNESS_DONE: &str = "harness-at-boot";

/// Binaries a build carries that live in another part's folder: the voice
/// starts its turn-end worker from its own (docs/endpoint-detection.md).
const BESIDE: &[(&str, &[&str])] = &[("lamp-voice", &["lamp-turn-end"])];

/// Services started by a timer of their own, and never enabled themselves.
const TIMED: &[&str] = &["lamp-records.service", "lamp-selftest.service"];

/// The units enabled: the target, and what starts outside it.
fn enabled() -> Vec<String> {
    PRODUCT
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| name.ends_with(".service") && !TIMED.contains(name))
        .chain([
            "lampos.target",
            "lamp-records.timer",
            "lamp-selftest.timer",
            "lampos-setup-done.path",
        ])
        .map(str::to_owned)
        .collect()
}

/// What a product unit starts: `/opt/lampOS/<part>/current/<binary>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The part's folder: `/opt/lampOS/voice/`.
    pub folder: String,
    /// `lamp-voice`.
    pub binary: String,
}

/// The part a unit's first `ExecStart=` starts through its `current` link.
pub fn part(text: &str) -> Option<Part> {
    unit::linked(text).map(|(folder, binary)| Part { folder, binary })
}

/// Every part of the product, in the order of `PRODUCT`, each once: setup
/// and its reset start the same binary.
pub fn parts() -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in PRODUCT.iter().filter_map(|(_, text)| part(text)) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

/// What `install` asks of the machine, behind a trait so that the tests
/// install onto a folder.
pub trait Host {
    /// A system user with its own group, no login and no home, in `groups`.
    fn add_user(&mut self, name: &str, groups: &[String]) -> Result<(), String>;
    /// `systemctl daemon-reload`, then `systemctl enable` each unit.
    fn enable(&mut self, units: &[String]) -> Result<(), String>;
    /// `systemd-tmpfiles --create` of one file.
    fn tmpfiles(&mut self, path: &str) -> Result<(), String>;
    /// The zone's offset from UTC now, in minutes.
    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String>;
    /// Harness at boot for `user` (#811, the first builder's recipe of
    /// 01:56): `home` made the user's own, `loginctl enable-linger`, then
    /// the Harness CLI's own `harness service install` as `user` with that
    /// `HOME`. Never a unit of this program's writing.
    fn harness_at_boot(&mut self, user: &str, home: &str) -> Result<(), String>;
}

/// The machine itself: only with the root `/`.
pub struct System;

impl Host for System {
    fn add_user(&mut self, name: &str, groups: &[String]) -> Result<(), String> {
        let mut command = Command::new("useradd");
        command.args(["--system", "--user-group", "--no-create-home"]);
        command.args(["--home-dir", "/nonexistent", "--shell", "/usr/sbin/nologin"]);
        if !groups.is_empty() {
            command.args(["--groups", &groups.join(",")]);
        }
        run(command.arg(name))
    }

    fn enable(&mut self, units: &[String]) -> Result<(), String> {
        run(Command::new("systemctl").arg("daemon-reload"))?;
        run(Command::new("systemctl").arg("enable").args(units))
    }

    fn tmpfiles(&mut self, path: &str) -> Result<(), String> {
        run(Command::new("systemd-tmpfiles").args(["--create", path]))
    }

    fn harness_at_boot(&mut self, user: &str, home: &str) -> Result<(), String> {
        run(Command::new("install").args(["-d", "-o", user, "-g", user, "-m", "0700", home]))?;
        // A user manager for the user from boot, with no login: the
        // daemon's user unit needs it, a system user as any other.
        run(Command::new("loginctl").args(["enable-linger", user]))?;
        let output = Command::new("id")
            .args(["-u", user])
            .output()
            .map_err(|error| format!("id: {error}"))?;
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("id -u {user} gave no user id"));
        }
        run(Command::new("runuser")
            .args(["-u", user, "--", "env"])
            .arg(format!("HOME={home}"))
            .arg(format!("XDG_RUNTIME_DIR=/run/user/{uid}"))
            .args([HARNESS, "service", "install"]))
    }

    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String> {
        let output = Command::new("date")
            .arg("+%z")
            .env("TZ", zone)
            .output()
            .map_err(|error| format!("date: {error}"))?;
        offset_minutes(String::from_utf8_lossy(&output.stdout).trim())
            .ok_or_else(|| format!("date gave no offset for {zone}"))
    }
}

fn run(command: &mut Command) -> Result<(), String> {
    let shown = format!("{command:?}");
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "{shown}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("{shown}: {error}")),
    }
}

/// `+0700` or `-0330` as minutes.
pub fn offset_minutes(text: &str) -> Option<i32> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

pub struct Install {
    /// The filesystem the lamp's paths are under: `/` on a lamp.
    pub root: PathBuf,
    pub channel: String,
    pub channel_url: String,
    /// The image's public key.
    pub key: [u8; 32],
    /// Where lamp-update keeps its records: `/var/lib/lampos-update/`.
    pub records: String,
    /// `--setup on`: also write setup's settings ([`SETUP_SETTINGS`]).
    pub setup: bool,
    /// The models to place where missing: [`models`] on a lamp.
    pub models: Vec<Model>,
}

impl Install {
    fn on(&self, path: &str) -> PathBuf {
        self.root
            .join(path.trim_start_matches('/').trim_end_matches('/'))
    }
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub sequence: u64,
    /// One line for each thing written or made; empty when nothing was.
    pub changed: Vec<String>,
}

/// Installs the newest build of the channel and the product around it.
pub fn install(
    config: &Install,
    channel: &mut dyn Channel,
    host: &mut dyn Host,
    say: &mut dyn FnMut(&str),
) -> Result<Installed, String> {
    let manifest = newest(config, channel)?;
    let records = Records::new(config.on(&config.records));
    let _lock = records
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    // The records as `Records::state` reads them: an unreadable file stops
    // install rather than being written over, and a corrupt one is set
    // aside and said. Set aside, it is not known what install wrote before,
    // so V1's key is not copied (below): a reset may have wiped the user's
    // key, and V1's config may still hold the previous owner's (#1094).
    let state_file = config.on(&config.records).join("state.json");
    let existed = state_file.exists();
    let mut state = records
        .state(say)
        .map_err(|error| format!("the state cannot be read: {error}"))?;
    let records_lost = records_lost(&config.on(&config.records), existed);
    let mut changed = Vec::new();

    // The users first: a folder or a part made below may name them.
    let groups = existing_groups(config, GROUPS);
    for missing in GROUPS
        .iter()
        .filter(|group| !groups.iter().any(|g| g == *group))
    {
        say(&format!(
            "the image has no group {missing}; {USER} is not put in it"
        ));
    }
    for (user, groups) in [(USER, groups), (MEMORY_USER, vec![USER.to_owned()])] {
        if !has_user(config, user) {
            host.add_user(user, &groups)?;
            changed.push(format!("made the user {user}"));
        }
    }

    // Every part's build beside nothing, then its link.
    let mut wanted = Vec::new();
    for part in parts() {
        let mut names = vec![part.binary.clone()];
        if let Some((_, beside)) = BESIDE.iter().find(|(binary, _)| *binary == part.binary) {
            names.extend(beside.iter().map(|name| (*name).to_owned()));
        }
        let files: Vec<&manifest::File> = names
            .iter()
            .map(|name| {
                manifest
                    .files
                    .iter()
                    .find(|file| &file.name == name)
                    .ok_or_else(|| format!("build {} carries no {name}", manifest.sequence))
            })
            .collect::<Result<_, _>>()?;
        wanted.push((part, files));
    }
    // Every folder whole before any link moves: a file that fails leaves
    // the folders made so far beside, unnamed by any link, for the next run.
    let mut builds = Vec::new();
    let mut made = Vec::new();
    for (part, files) in &wanted {
        let before = changed.len();
        let build = place(config, &manifest, channel, part, files, &mut changed)?;
        if changed.len() > before {
            made.push(unit::folder(&part.folder, &build));
        }
        builds.push(build);
    }
    for ((part, _), build) in wanted.iter().zip(&builds) {
        link(config, part, build, &mut changed)?;
    }

    // The units and the tmpfiles. systemd must be told of any of them
    // written, so that is recorded before the first is: a run stopped
    // anywhere after leaves the debt for the next (the reviewer on #815,
    // 19:17 and 19:32).
    let for_systemd: Vec<(String, &[u8])> = PRODUCT
        .iter()
        .map(|(name, text)| (format!("{UNITS}{name}"), text.as_bytes()))
        .chain([(TMPFILES_PATH.to_owned(), TMPFILES.as_bytes())])
        .collect();
    if !state.systemd_owed
        && for_systemd
            .iter()
            .any(|(path, bytes)| fs::read(config.on(path)).map_or(true, |now| now != *bytes))
    {
        state.systemd_owed = true;
        save(&records, &state)?;
    }
    for (path, bytes) in &for_systemd {
        put(config, path, bytes, 0o644, &mut changed)?;
    }
    let mut defaults = SETTINGS.to_vec();
    if config.setup {
        defaults.extend_from_slice(SETUP_SETTINGS);
    }
    place_models(
        config,
        channel,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    settings(
        config,
        &defaults,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    // The voice's service and the user's key, private. Each is install's
    // while it is still what an install wrote, so a newer provider, or a key
    // V1 rotated in its config, reaches the lamp; one setup wrote, or one
    // changed on the lamp, is kept (the reviewer, thirteen, on #1050). The key
    // is the user's (the owner on #806, 16:13): V1's is copied only into a lamp
    // that never had one from install. One install wrote that is now gone was
    // removed on purpose (a reset wipes it with the person's data), and V1's
    // config, which may still hold the previous owner's, never puts it back.
    let key = device_key(config);
    let mut private: Vec<(&str, &str)> = vec![(PROVIDER_PATH, PROVIDER)];
    let mut marked = false;
    for (path, part) in [(CREDENTIAL_PATH, "the voice"), (JEV_KEY_PATH, "the Policy")] {
        let there = config.on(path).exists();
        let had = state.settings.contains_key(path);
        // Every key path, the ones there too: a key there now is removed by
        // a later reset, and must not come back then. One there that is
        // V1's gets its real hash below; any other is kept as the lamp's own
        // (the reviewer on #1263).
        if records_lost {
            state.settings.insert(path.to_owned(), SET_ASIDE.to_owned());
            marked = true;
        }
        match &key {
            Some(key) if there || (!had && !records_lost) => private.push((path, key)),
            _ if there => {}
            Some(_) if records_lost => say(&format!(
                "{path} is missing and the records were set aside: V1's key is not copied; setup takes the user's"
            )),
            Some(_) => say(&format!(
                "{path} was removed since install wrote it (a reset?): V1's key is not put back; setup takes the user's"
            )),
            None => say(&format!(
                "no key: {part} cannot reach its service until setup takes the user's into {path} (none in {V1_CONFIG})"
            )),
        }
    }
    if marked {
        save(&records, &state)?;
    }
    let wrote = changed.len();
    settings_with_mode(
        config,
        &private,
        0o600,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    for path in [CREDENTIAL_PATH, JEV_KEY_PATH] {
        if changed[wrote..]
            .iter()
            .any(|line| line == &format!("wrote {path}"))
        {
            say(&format!("V1's key was taken from {V1_CONFIG} into {path}"));
        }
    }
    // Made 0755 here on a blank lamp. One made closed before (lamp-4ace's is
    // root's alone) is not opened: credentials may be below it. Said instead.
    if let Ok(meta) = fs::metadata(config.on("/etc/lampOS"))
        && meta.permissions().mode() & 0o001 == 0
    {
        say(&format!(
            "/etc/lampOS is closed to other users ({:o}): the parts run as {USER} cannot read their settings in it",
            meta.permissions().mode() & 0o777
        ));
    }
    // Written when missing, and again when the lamp's saved zone is another
    // (#820): a lamp set up before setup wrote this file kept the image's
    // zone here, which schedule and skills read, while memory and Expression
    // follow /etc/localtime. A lamp naming no zone keeps the file it has.
    let saved = saved_zone(config);
    let written = fs::read_to_string(config.on(ZONE_PATH)).ok();
    let written_zone = written.as_deref().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("LAMP_ZONE="))
            .map(str::trim)
    });
    let differs = written.is_some()
        && saved
            .as_deref()
            .is_some_and(|zone| written_zone != Some(zone));
    // Only to a zone the lamp has: a wrong /etc/timezone must not replace a
    // good file, or schedule would not start on it (the reviewer on #1380).
    let stale = differs
        && saved
            .as_deref()
            .is_some_and(|zone| zone_file_known(config, zone));
    if differs && !stale {
        say(&format!(
            "kept {ZONE_PATH}: the lamp names {} as its zone, which is not a zone in /usr/share/zoneinfo",
            saved.as_deref().unwrap_or_default()
        ));
    }
    if written.is_none() || stale {
        let zone = saved.unwrap_or_else(|| "UTC".to_owned());
        let offset = host.utc_offset_minutes(&zone)?;
        if stale {
            say(&format!(
                "{ZONE_PATH} follows the zone this lamp was set up in, {zone}"
            ));
        }
        let text = format!(
            "# The lamp's zone, for schedule and skills (docs/install.md).\n\
             # Written by lamp-update install from the system's zone; setup writes it again.\n\
             LAMP_ZONE={zone}\nLAMP_UTC_OFFSET_MINUTES={offset}\n"
        );
        put(config, ZONE_PATH, text.as_bytes(), 0o644, &mut changed)?;
    }
    if !config.on("/etc/lampOS/body/lamp.env").exists() {
        say(
            "the body waits for this lamp's own poses in /etc/lampOS/body/lamp.env (docs/install.md)",
        );
    }

    let before = state.clone();
    let build = Build {
        sequence: manifest.sequence,
        commit: manifest.commit.clone(),
    };
    // The folders made are this program's: a round removes one once
    // neither the link nor the build before names it.
    let new_folders: Vec<String> = made
        .into_iter()
        .filter(|folder| !state.made.contains(folder))
        .collect();
    if state.installed.as_ref() != Some(&build) {
        state.previous = state.installed.replace(build);
    }
    state.made.extend(new_folders);
    // Everything on disk is recorded before systemd is asked anything,
    // with what is owed to it: a run whose enable fails leaves the debt,
    // and the next run pays it though it writes nothing (the reviewer on
    // #815, 19:17).
    state.systemd_owed |= !changed.is_empty();
    if state != before {
        save(&records, &state)?;
    }

    // Started by nothing: enabled, for the next power-up after setup.
    if state.systemd_owed {
        host.tmpfiles(TMPFILES_PATH)?;
        host.enable(&enabled())?;
        changed.push(format!("enabled {}", enabled().join(" ")));
        state.systemd_owed = false;
        save(&records, &state)?;
    }
    // Harness at boot, once it is on the lamp; not having it is said, not
    // a failure: it is installed by its own installer (lamp-tasks.service).
    if !config.on(HARNESS).exists() {
        say(&format!(
            "Harness is not at {HARNESS}: it is not set to start at boot"
        ));
    } else {
        let done = config.on(&config.records).join(HARNESS_DONE);
        if !done.exists() {
            // A failure leaves nothing recorded: the next install asks again.
            host.harness_at_boot(USER, HARNESS_HOME)?;
            write_whole(&done, format!("{USER} {HARNESS_HOME}\n").as_bytes(), 0o644)
                .map_err(|error| format!("{}: {error}", done.display()))?;
            changed.push(format!("Harness starts at boot as {USER}"));
        }
    }
    for line in &changed {
        say(line);
    }
    Ok(Installed {
        sequence: manifest.sequence,
        changed,
    })
}

fn save(records: &Records, state: &State) -> Result<(), String> {
    records
        .save_state(state)
        .map_err(|error| format!("the state cannot be written: {error}"))
}

/// The product's settings, from `defaults` (path, bytes): each written
/// where it is missing, or where it is still exactly what an install wrote
/// before (by its SHA-256 in the state's `settings`, or in
/// `settings_writing` when a run stopped between writing it and recording
/// it), so a newer build's default reaches a lamp that never changed it.
/// One that differs from all of these was changed on the lamp, or was
/// there before any install (a lamp's own pins, its calibration): it is
/// kept, and said, and its record is left as it was. Rerunning install is
/// also how the updater upgrades itself, so this holds at every run (the
/// reviewer on #815).
///
/// What is about to be written is recorded through `persist` before the
/// first write, and what was written after the last, so that no stop in
/// between leaves a file of install's that the next run takes for the
/// lamp's.
pub fn settings(
    config: &Install,
    defaults: &[(&str, &str)],
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    settings_with_mode(config, defaults, 0o644, state, changed, say, persist)
}

/// [`settings`], written with `mode`: the voice's service and the device
/// key are private (0600), as the voice requires.
pub fn settings_with_mode(
    config: &Install,
    defaults: &[(&str, &str)],
    mode: u32,
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    let before = state.clone();
    let mut writes = Vec::new();
    for (path, text) in defaults {
        let default = manifest::sha256_hex(text.as_bytes());
        let now = match fs::read(config.on(path)) {
            Ok(bytes) => Some(manifest::sha256_hex(&bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("{path}: {error}")),
        };
        let installs = |hash: &String| {
            state.settings.get(*path) == Some(hash)
                || state.settings_writing.get(*path) == Some(hash)
        };
        match now {
            Some(now) if now == default => {
                // Its bytes are install's, so its mode is too: a default
                // written closed by something before install (lamp-cb94's,
                // 0600) is opened, or the parts run as `lampos` never read it.
                set_mode(config, path, text.as_bytes(), mode, changed)?;
                state.settings.insert((*path).to_owned(), default);
            }
            Some(now) if !installs(&now) => {
                say(&format!(
                    "kept {path}: this lamp's own, not a default an install wrote; the build's default is not written"
                ));
                // The lamp's own is not install's to open: said instead,
                // and only said, so a file gone since it was read stops
                // nothing (the reviewer on #1293).
                if mode & 0o004 != 0
                    && let Ok(meta) = fs::metadata(config.on(path))
                    && meta.permissions().mode() & 0o004 == 0
                {
                    say(&format!(
                        "{path} is closed to other users ({:o}): the parts run as {USER} cannot read it",
                        meta.permissions().mode() & 0o777
                    ));
                }
            }
            _ => {
                // A file that is install's only by an earlier run's intent
                // (written, then that run failed or stopped) is settled as
                // install's now, before this run's intent takes the
                // intent's place: a second failure must not lose it (the
                // reviewer on #815, 21:42).
                if let Some(now) = now {
                    state.settings.insert((*path).to_owned(), now);
                }
                writes.push((*path, *text, default));
            }
        }
    }
    if !writes.is_empty() {
        for (path, _, default) in &writes {
            state
                .settings_writing
                .insert((*path).to_owned(), default.clone());
        }
        persist(state)?;
        for (path, text, default) in writes {
            put(config, path, text.as_bytes(), mode, changed)?;
            state.settings.insert(path.to_owned(), default);
        }
    }
    // Every setting is settled: nothing is being written any more.
    state.settings_writing.clear();
    if *state != before {
        persist(state)?;
    }
    Ok(())
}

/// The channel's newest build, checked with the image's key.
fn newest(config: &Install, channel: &mut dyn Channel) -> Result<Manifest, String> {
    let url = format!("{}{}", config.channel_url, manifest::ENVELOPE);
    let mut envelope = Vec::new();
    match channel.fetch(&url, manifest::MOST_ENVELOPE_BYTES, &mut envelope) {
        Ok(_) => {}
        Err(Fetch::Missing) => return Err("the channel has published no build".into()),
        Err(error) => return Err(error.to_string()),
    }
    let (manifest, _) = manifest::open(&envelope, &config.key).map_err(|why| why.0)?;
    if manifest.channel != config.channel {
        return Err(format!(
            "the build is of channel {}, not {}",
            manifest.channel, config.channel
        ));
    }
    Ok(manifest)
}

/// The part's build folder, filled if it is not there whole: named by the
/// binary's SHA-256, or, when a folder of that name holds other versions of
/// what goes beside it, by the SHA-256 of their list (as a round names it).
fn place(
    config: &Install,
    manifest: &Manifest,
    channel: &mut dyn Channel,
    part: &Part,
    files: &[&manifest::File],
    changed: &mut Vec<String>,
) -> Result<String, String> {
    let mut lines: Vec<String> = files
        .iter()
        .map(|file| format!("{}  {}\n", file.sha256, file.name))
        .collect();
    lines.sort();
    let names = [
        files[0].sha256.clone(),
        manifest::sha256_hex(lines.concat().as_bytes()),
    ];
    for (index, name) in names.iter().enumerate() {
        let folder = config.on(&format!("{}{name}", part.folder));
        if !folder.exists() {
            fill(manifest, channel, &folder, files)?;
            changed.push(format!("installed {}{name}/", part.folder));
            return Ok(name.clone());
        }
        if holds(&folder, files) {
            return Ok(name.clone());
        }
        if files.len() == 1 || index == 1 {
            return Err(format!(
                "{} is there and does not hold what build {} has; it is left as it is",
                folder.display(),
                manifest.sequence
            ));
        }
    }
    unreachable!("the second name returns")
}

fn holds(folder: &Path, files: &[&manifest::File]) -> bool {
    files
        .iter()
        .all(|file| file_sha256(&folder.join(&file.name)).is_ok_and(|sha| sha == file.sha256))
}

/// One folder of a build, filled under its temporary name and renamed only
/// when every file is whole and matches.
fn fill(
    manifest: &Manifest,
    channel: &mut dyn Channel,
    folder: &Path,
    files: &[&manifest::File],
) -> Result<(), String> {
    let failed = |error: io::Error| format!("{}: {error}", folder.display());
    let temporary = filling(folder);
    if let Some(parent) = folder.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(failed)?;
    }
    if temporary.exists() {
        fs::remove_dir_all(&temporary).map_err(failed)?;
    }
    fs::DirBuilder::new()
        .mode(0o755)
        .create(&temporary)
        .map_err(failed)?;
    let result = (|| {
        for file in files {
            let url = format!("{}{}", manifest.files_url, file.name);
            let mut out = fs::File::create(temporary.join(&file.name)).map_err(failed)?;
            let mut hashing = Hashing {
                file: &mut out,
                context: digest::Context::new(&digest::SHA256),
            };
            let fetched = channel.fetch(&url, file.size, &mut hashing);
            let sha = manifest::hex(hashing.context.finish().as_ref());
            match fetched {
                Ok(size) if size == file.size && sha == file.sha256 => {}
                Ok(_) => return Err(format!("{} does not match its SHA-256", file.name)),
                Err(error) => return Err(format!("{}: {error}", file.name)),
            }
            out.set_permissions(fs::Permissions::from_mode(0o755))
                .and_then(|()| out.sync_all())
                .map_err(failed)?;
        }
        fs::rename(&temporary, folder)
            .and_then(|()| sync_parent(folder))
            .map_err(failed)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

/// `current` pointing at the build, replaced whole if it points elsewhere.
fn link(
    config: &Install,
    part: &Part,
    build: &str,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let path = unit::current(&part.folder);
    let current = config.on(&path);
    if fs::read_link(&current).is_ok_and(|target| target == Path::new(build)) {
        return Ok(());
    }
    replace_link(&current, build).map_err(|error| format!("{path}: {error}"))?;
    changed.push(format!("{path} -> {build}"));
    Ok(())
}

/// A file written whole, only if it is not already exactly this.
fn put(
    config: &Install,
    path: &str,
    bytes: &[u8],
    mode: u32,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let at = config.on(path);
    if fs::read(&at).is_ok_and(|now| now == bytes) {
        return set_mode(config, path, bytes, mode, changed);
    }
    let failed = |error: io::Error| format!("{path}: {error}");
    if let Some(parent) = at.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(failed)?;
    }
    write_whole(&at, bytes, mode).map_err(failed)?;
    changed.push(format!("wrote {path}"));
    Ok(())
}

/// Gives install's file at `path` `mode` when it has another and still
/// holds exactly `bytes`: the bytes alone do not make it install's as
/// written (the first builder on #705, 12:13). It is opened once, never
/// through a link, and what is read and what is changed are that one open
/// file, so nothing swapped in at the path between is touched (the
/// reviewer on #1293). A link is left as it is: its target is not
/// install's to open or close (Nine on #1293).
fn set_mode(
    config: &Install,
    path: &str,
    bytes: &[u8],
    mode: u32,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let failed = |error: io::Error| format!("{path}: {error}");
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(config.on(path))
    {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            return Ok(());
        }
        Err(error) => return Err(failed(error)),
    };
    let meta = file.metadata().map_err(failed)?;
    if !meta.is_file() {
        return Ok(());
    }
    let mut now_bytes = Vec::new();
    file.read_to_end(&mut now_bytes).map_err(failed)?;
    if now_bytes != bytes {
        return Ok(());
    }
    let now = meta.permissions().mode() & 0o7777;
    if now != mode {
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(failed)?;
        changed.push(format!("made {path} {mode:o}, was {now:o}"));
    }
    Ok(())
}

/// V1's device key (`llm_api_key` in its config), when it is one line of
/// printable text a header can carry. Never said or logged.
fn device_key(config: &Install) -> Option<String> {
    let text = fs::read_to_string(config.on(V1_CONFIG)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let key = value.get("llm_api_key")?.as_str()?;
    (!key.is_empty() && key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| key.to_owned())
}

/// Each model missing on the lamp, fetched and checked by size and SHA-256
/// under a temporary name, then renamed into place. One that cannot be had
/// is said, and the install goes on: its part cannot start until it is
/// there, and the next install tries again. A file already there that is not
/// the pinned one is the lamp's own (lamp-52e6's own `name.onnx`): kept, and
/// said.
fn place_models(
    config: &Install,
    channel: &mut dyn Channel,
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    let before = state.clone();
    for model in &config.models {
        let at = config.on(&model.path);
        match file_sha256(&at) {
            Ok(sha) if sha == model.sha256 => {
                state.settings.insert(model.path.clone(), sha);
                continue;
            }
            // An earlier pin this program placed (its SHA-256 recorded as
            // the settings' are): replaced, since its part checks the new
            // pin and would refuse it (the reviewer, thirteen, on #1052).
            Ok(sha) if state.settings.get(&model.path) == Some(&sha) => {}
            Ok(_) => {
                say(&format!("kept {}: it is not the pinned model", model.path));
                continue;
            }
            Err(_) => {}
        }
        match fetch_model(channel, model, &at) {
            Ok(()) => {
                state
                    .settings
                    .insert(model.path.clone(), model.sha256.clone());
                changed.push(format!("fetched {}", model.path));
            }
            Err(why) => say(&format!(
                "{} could not be had ({why}); its part cannot start until it is there",
                model.path
            )),
        }
    }
    if *state != before {
        persist(state)?;
    }
    Ok(())
}

fn fetch_model(channel: &mut dyn Channel, model: &Model, at: &Path) -> Result<(), String> {
    let parent = at.parent().ok_or("no folder")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(parent)
        .map_err(|error| error.to_string())?;
    let name = at.file_name().ok_or("no name")?.to_string_lossy();
    let temporary = parent.join(format!(".{name}.fetching"));
    let archive = parent.join(format!(".{name}.archive"));
    let result = (|| {
        match &model.archive {
            None => fetch_checked(channel, &model.url, model.bytes, &model.sha256, &temporary)?,
            Some(pinned) => {
                fetch_checked(channel, &model.url, pinned.bytes, &pinned.sha256, &archive)?;
                let out = fs::File::create(&temporary).map_err(|error| error.to_string())?;
                let status = Command::new("tar")
                    .arg("-xzOf")
                    .arg(&archive)
                    .arg(&pinned.member)
                    .stdout(out)
                    .status()
                    .map_err(|error| format!("tar: {error}"))?;
                if !status.success() {
                    return Err(format!("tar could not take {} out", pinned.member));
                }
                let sha = file_sha256(&temporary).map_err(|error| error.to_string())?;
                let size = fs::metadata(&temporary)
                    .map_err(|error| error.to_string())?
                    .len();
                if size != model.bytes || sha != model.sha256 {
                    return Err(format!("{} does not match its SHA-256", pinned.member));
                }
            }
        }
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))
            .and_then(|()| fs::File::open(&temporary)?.sync_all())
            .and_then(|()| fs::rename(&temporary, at))
            .and_then(|()| sync_parent(at))
            .map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(&archive);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// The file at `url` into `to`, only if it is `bytes` long with `sha256`.
fn fetch_checked(
    channel: &mut dyn Channel,
    url: &str,
    bytes: u64,
    sha256: &str,
    to: &Path,
) -> Result<(), String> {
    let mut out = fs::File::create(to).map_err(|error| error.to_string())?;
    let mut hashing = Hashing {
        file: &mut out,
        context: digest::Context::new(&digest::SHA256),
    };
    let fetched = channel.fetch(url, bytes, &mut hashing);
    let sha = manifest::hex(hashing.context.finish().as_ref());
    match fetched {
        Ok(size) if size == bytes && sha == sha256 => Ok(()),
        Ok(_) => Err("it does not match its SHA-256".into()),
        Err(Fetch::Missing) => Err(format!("{url} is not there")),
        Err(error) => Err(format!("{error:?}")),
    }
}

/// V1's key where V1 still owns the lamp, for `lamp-keeper prepare`
/// (docs/keeper.md): [`CREDENTIAL_PATH`] and [`JEV_KEY_PATH`] written by
/// [`settings_with_mode`], as [`install`] writes them (0600, whole), and
/// recorded in lamp-update's records (`records` under `root`) the same way,
/// so install and prepare share one rule. Only a file that is missing is
/// written, and only into a lamp that never had one from install or
/// prepare: one recorded and now gone was removed on purpose (a reset wipes
/// the user's key, #806), and V1's config, which may still hold the previous
/// owner's, never puts it back. The records are read as install reads them:
/// a corrupt file is set aside and said, and V1's key is then not copied;
/// one that cannot be read is said, and nothing is written. `root` is `/` on
/// a lamp. Returns the paths written; the key is never in what it returns or
/// says.
pub fn provision_device_key(
    root: &Path,
    records: &str,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<&'static str>, String> {
    let config = Install {
        root: root.to_owned(),
        channel: String::new(),
        channel_url: String::new(),
        key: [0; 32],
        records: records.to_owned(),
        setup: false,
        models: Vec::new(),
    };
    let records = Records::new(config.on(&config.records));
    let _lock = records
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    // The records as install reads them (`Records::state`, #1094's review):
    // a corrupt file is set aside and said, and then what was written
    // before is not known, so V1's key is not copied (a reset may have wiped
    // the user's). One that cannot be read is said and left alone, nothing
    // written: a refusal would refuse every lamp-os update.
    let state_file = config.on(&config.records).join("state.json");
    let existed = state_file.exists();
    let mut state = match records.state(say) {
        Ok(state) => state,
        Err(error) => {
            say(&format!(
                "the records cannot be read ({error}): V1's key is not written; install or setup sees to it"
            ));
            return Ok(Vec::new());
        }
    };
    let records_lost = records_lost(&config.on(&config.records), existed);
    let mut wanted = Vec::new();
    for path in [CREDENTIAL_PATH, JEV_KEY_PATH] {
        // Every key path is marked, the ones there too: a key there now is
        // removed by a later reset, and must not come back then (the
        // reviewer on #1263).
        if records_lost {
            state.settings.insert(path.to_owned(), SET_ASIDE.to_owned());
        }
        if fs::symlink_metadata(config.on(path)).is_ok() {
            continue;
        }
        if records_lost {
            say(&format!(
                "{path} is missing and the records were set aside: V1's key is not copied; setup takes the user's"
            ));
        } else if state.settings.contains_key(path) {
            say(&format!(
                "{path} was removed since it was written (a reset?): V1's key is not put back; setup takes the user's"
            ));
        } else {
            wanted.push(path);
        }
    }
    if records_lost {
        save(&records, &state)?;
    }
    if wanted.is_empty() {
        return Ok(wanted);
    }
    // No usable key: said, and left to setup, as install does; prepare runs
    // before every lamp-os update, so refusing here would refuse them all.
    let Some(key) = device_key(&config) else {
        for path in wanted {
            say(&format!(
                "no key: nothing written to {path} until setup takes the user's (none in {V1_CONFIG})"
            ));
        }
        return Ok(Vec::new());
    };
    let private: Vec<(&str, &str)> = wanted.iter().map(|path| (*path, key.as_str())).collect();
    let mut changed = Vec::new();
    settings_with_mode(
        &config,
        &private,
        0o600,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    Ok(wanted)
}

fn has_user(config: &Install, name: &str) -> bool {
    fs::read_to_string(config.on("/etc/passwd")).is_ok_and(|text| {
        text.lines()
            .any(|line| line.split(':').next() == Some(name))
    })
}

fn existing_groups(config: &Install, wanted: &[&str]) -> Vec<String> {
    let text = fs::read_to_string(config.on("/etc/group")).unwrap_or_default();
    wanted
        .iter()
        .filter(|group| {
            text.lines()
                .any(|line| line.split(':').next() == Some(**group))
        })
        .map(|group| (*group).to_owned())
        .collect()
}

/// Whether `zone` is a zone file in the lamp's zone data, as setup checks a
/// zone before it saves one (`zone_known` in `crates/setup`): a plain name
/// whose file, links followed, stays inside `/usr/share/zoneinfo` and is a
/// `TZif` file.
fn zone_file_known(config: &Install, zone: &str) -> bool {
    if zone.is_empty()
        || zone.len() > 64
        || !zone
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        || !zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return false;
    }
    let zoneinfo = config.on("/usr/share/zoneinfo");
    let (Ok(root), Ok(file)) = (zoneinfo.canonicalize(), zoneinfo.join(zone).canonicalize()) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.starts_with(&root)
        && fs::File::open(&file)
            .and_then(|mut opened| io::Read::read_exact(&mut opened, &mut magic))
            .is_ok()
        && &magic == b"TZif"
}

/// The zone the lamp names: `/etc/timezone`, which setup writes with the
/// person's zone, else where `/etc/localtime` links. None when neither says.
fn saved_zone(config: &Install) -> Option<String> {
    if let Ok(text) = fs::read_to_string(config.on("/etc/timezone")) {
        let zone = text.trim();
        if !zone.is_empty() {
            return Some(zone.to_owned());
        }
    }
    fs::read_link(config.on("/etc/localtime"))
        .ok()
        .and_then(|target| {
            let target = target.to_string_lossy().into_owned();
            target
                .split_once("zoneinfo/")
                .map(|(_, zone)| zone.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_product_part_starts_through_its_current_link() {
        let parts = parts();
        for (name, text) in PRODUCT
            .iter()
            .filter(|(name, _)| name.ends_with(".service"))
        {
            assert!(part(text).is_some(), "{name} starts through a current link");
        }
        // Setup and its reset share one binary, and so do the air and its
        // bus clock; every other service is a part.
        let services = PRODUCT
            .iter()
            .filter(|(name, _)| name.ends_with(".service"))
            .count();
        assert_eq!(parts.len(), services - 2, "every product service is a part");
        for part in &parts {
            let name = part.binary.trim_start_matches("lamp-");
            assert_eq!(part.folder, format!("/opt/lampOS/{name}/"), "{part:?}");
        }
    }

    /// #812: the passive self-test is started by its timer alone, which the
    /// lamp's target wants, as the records' check is by its own.
    #[test]
    fn a_timed_service_is_never_enabled_and_its_timer_is() {
        let enabled = enabled();
        for timed in TIMED {
            assert!(!enabled.iter().any(|name| name == timed), "{timed}");
            let timer = timed.replace(".service", ".timer");
            assert!(enabled.contains(&timer), "{timer}");
            assert!(PRODUCT.iter().any(|(name, _)| *name == timer), "{timer}");
        }
        let (_, timer) = PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-selftest.timer")
            .unwrap();
        assert!(timer.contains("WantedBy=lampos.target"), "{timer}");
        let (_, service) = PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-selftest.service")
            .unwrap();
        assert!(!service.contains("[Install]"), "{service}");
        assert!(service.contains("--occasion first-boot"), "{service}");
    }

    #[test]
    fn no_product_unit_names_a_lamp() {
        for (name, text) in PRODUCT {
            // What systemd reads; a comment may say where a value was measured.
            let settings: String = text
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .collect();
            assert!(!settings.contains("ConditionHost"), "{name}");
            // A template's placeholder is @UPPER_CASE@; the camera's USB path has an @ too.
            let placeholder = settings.split('@').skip(1).step_by(2).any(|piece| {
                !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            });
            assert!(!placeholder, "{name} has a placeholder");
            assert!(!settings.contains("lamp-4ace"), "{name} names a lamp");
            assert!(!settings.contains("lamp-52e6"), "{name} names a lamp");
            assert!(!settings.contains("/home/"), "{name} names a home");
            assert!(
                crate::unit::program(text).is_none(),
                "{name} names a build's folder"
            );
        }
    }

    #[test]
    fn a_unit_through_current_is_read() {
        let text = "[Service]\nExecStart=-/opt/lampOS/body/current/lamp-body --x\n";
        assert_eq!(
            part(text),
            Some(Part {
                folder: "/opt/lampOS/body/".into(),
                binary: "lamp-body".into()
            })
        );
        for text in [
            "ExecStart=/opt/lampOS/body/lamp-body\n",
            "ExecStart=/opt/lampOS/body/latest/lamp-body\n",
            "ExecStart=/opt/lampOS/a/b/current/lamp-body\n",
            "ExecStart=/opt/lampOS/body/current/sh\n",
        ] {
            assert_eq!(part(text), None, "{text}");
        }
    }

    #[test]
    fn offsets_are_read() {
        assert_eq!(offset_minutes("+0700"), Some(420));
        assert_eq!(offset_minutes("-0330"), Some(-210));
        assert_eq!(offset_minutes("+0000"), Some(0));
        assert_eq!(offset_minutes("0700"), None);
        assert_eq!(offset_minutes("+07"), None);
    }
}
