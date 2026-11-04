//! Harness put on a lamp before its sign-in (#5197; docs/install.md,
//! "Harness before the sign-in"), off unless `LAMP_HARNESS_AT_ONBOARDING=on`
//! in lamp-update's own unit.
//!
//! After each `serve` round that reached the channel (so the first one once
//! the onboarding has the lamp on Wi-Fi, and every later one), a lamp with
//! no Harness at [`install::HARNESS`] gets it, by the same steps as
//! `lamp-update harness-on` ([`install::harness_on_pinned_with`]), but
//! installed only: no Harness at boot and no daemon. lamp-tasks starts the
//! daemon after the person's sign-in, as it does today.
//!
//! A round's own sandbox cannot write `/usr/local/bin` or Harness's home
//! (the 7071 sandbox, `the_updaters_own_sandbox_is_still_the_7071_images`),
//! so the round writes the fixed one-shot [`UNIT`] and starts it without
//! waiting, as it does the settings writer's ([`crate::writer_unit`]); the
//! round and the onboarding never wait on Harness's download. The one-shot
//! runs `lamp-update harness-install` ([`install_now`]).
//!
//! What the app reads is [`RECORD`] in lamp-update's records, said on
//! `update/status` as `harness` ([`lamp_msgs::HarnessInstall`]), which
//! lamp-cloud's `update.status` answers. A failed try is tried again after
//! the next round, which the app's `update.check` starts at once.
//!
//! **Pinning (#3693's decision (a): a sha256 pin is enough):** with
//! `LAMP_HARNESS_MANIFEST_URL` and `LAMP_HARNESS_MANIFEST_SHA256` both set,
//! the manifest at that https URL is fetched and its sha256 compared first;
//! one that differs installs nothing (`pin_mismatch`). A match runs
//! Harness's installer with `HARNESS_METADATA_URL` at those checked bytes,
//! served on 127.0.0.1 while it runs ([`serve`], #5229), so the installer
//! checks what it fetches against the pinned manifest's sha256s and never
//! reads the manifest from its server again. With neither set, the
//! install is as `harness-on`'s (Harness's own CDN manifest) and is said
//! `pinned: false`, except on a lamp installed as the product, which takes
//! the pin compiled into this build, covered by its signature, and never
//! installs unpinned ([`product`], #5229).
use crate::{
    install::{self, HARNESS, HARNESS_HOME, Runner, USER},
    records::write_whole,
    world::Systemd,
};
use lamp_msgs::HarnessInstall;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Duration,
};

/// The switch, in lamp-update's own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_HARNESS_AT_ONBOARDING";
/// The pinned Harness manifest's https URL, with [`MANIFEST_SHA256`].
pub const MANIFEST_URL: &str = "LAMP_HARNESS_MANIFEST_URL";
/// The pinned Harness manifest's sha256, 64 hex digits.
pub const MANIFEST_SHA256: &str = "LAMP_HARNESS_MANIFEST_SHA256";

/// Where the install is, in lamp-update's records (0600).
pub const RECORD: &str = "harness-install.json";
/// What the round hands the one-shot: the pin, or none.
pub const ASK: &str = "harness-ask.json";
/// One Harness install at a time, apart from the round's own lock.
pub const LOCK: &str = "harness.lock";
/// An `installing` older than this is taken as ended without a word (the
/// one-shot killed past its `TimeoutStartSec`, or the lamp lost power)
/// and tried again.
pub const STUCK_AFTER: Duration = Duration::from_secs(30 * 60);
/// The largest pinned manifest fetched.
const MANIFEST_MOST_BYTES: &str = "1048576";

/// The one-shot unit. Fixed: a round passes no other name to systemd here.
pub const UNIT: &str = "lamp-update-harness.service";
/// The unit. Not `ProtectSystem=strict`: Harness's home
/// (`/var/lib/lampos-tasks`) may not be there yet, and is made under
/// `/var/lib`. `/etc` and `/usr` stay read-only but for `/usr/local/bin`,
/// where [`install::HARNESS`] is linked. Its bound is past the installer's
/// own ([`install::INSTALLER_WITHIN`]) and the steps around it.
pub const TEXT: &str = "\
# lamp-update-harness: puts Harness on the lamp before its sign-in, with Harness's own installer, installed only
# (lampOS/docs/install.md, \"Harness before the sign-in\", #5197). Written and started by a round of lamp-update
# with LAMP_HARNESS_AT_ONBOARDING=on, whose own sandbox cannot write /usr/local/bin or Harness's home; never
# enabled. Outside lampos.target, as lamp-update is.
[Unit]
Description=lampOS updates, Harness put on the lamp (lamp-update-harness)
After=network-online.target

[Service]
Type=oneshot
User=root
Group=root
ExecStart=/opt/lampOS/update/current/lamp-update harness-install
StandardInput=null
SyslogIdentifier=lamp-update-harness
TimeoutStartSec=1500
NoNewPrivileges=yes
ProtectSystem=full
ReadWritePaths=/usr/local/bin
ProtectHome=yes
PrivateTmp=yes
";

/// `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A pinned Harness manifest: where, and its sha256 (lower-case hex).
/// `built_in`: the copy compiled into this build ([`product::MANIFEST`]),
/// never fetched; `url` is then only where it was copied from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub url: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub built_in: bool,
}

/// The pin from [`MANIFEST_URL`] and [`MANIFEST_SHA256`]: none with
/// neither; refused with one alone, a URL that isn't plain https, or a
/// sha256 that isn't 64 hex digits.
pub fn pin_from(url: Option<&str>, sha256: Option<&str>) -> Result<Option<Pin>, String> {
    match (url, sha256) {
        (None, None) => Ok(None),
        (Some(url), Some(sha256)) => {
            let plain = url.starts_with("https://")
                && url.len() <= 2048
                && url.bytes().all(|b| b.is_ascii_graphic());
            if !plain {
                return Err(format!("{MANIFEST_URL} is not a plain https URL"));
            }
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("{MANIFEST_SHA256} is not 64 hex digits"));
            }
            Ok(Some(Pin {
                url: url.to_owned(),
                sha256: sha256.to_ascii_lowercase(),
                built_in: false,
            }))
        }
        _ => Err(format!("{MANIFEST_URL} and {MANIFEST_SHA256} go together")),
    }
}

/// What the round hands the one-shot in [`ASK`].
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ask {
    version: u32,
    pin: Option<Pin>,
}

/// The install as [`RECORD`] has it, when it is one lamp-update could have
/// written ([`HarnessInstall::check`]); else none.
pub fn record(records: &Path) -> Option<HarnessInstall> {
    let bytes = fs::read(records.join(RECORD)).ok()?;
    serde_json::from_slice::<HarnessInstall>(&bytes)
        .ok()
        .filter(HarnessInstall::check)
}

/// `said` written as [`RECORD`], its `revision` one past the record's
/// before (1 with none): each write is a change, and a read never writes.
/// What was written is returned; one that cannot be written is said again
/// at the next change.
fn write(records: &Path, mut said: HarnessInstall) -> HarnessInstall {
    said.revision = record(records).map_or(1, |before| before.revision.saturating_add(1));
    if let Ok(text) = serde_json::to_vec(&said) {
        let _ = write_whole(&records.join(RECORD), &text, 0o600);
    }
    said
}

fn state(install: &str, at_unix_ms: u64) -> HarnessInstall {
    HarnessInstall::new(install, None, at_unix_ms)
}

fn failed(at_unix_ms: u64, pinned: Option<bool>, error: &str) -> HarnessInstall {
    HarnessInstall {
        pinned,
        ..HarnessInstall::new("failed", Some(error), at_unix_ms)
    }
}

fn present(root: &Path) -> bool {
    fs::symlink_metadata(root.join(HARNESS.trim_start_matches('/'))).is_ok()
}

/// When `serve` starts: with the switch off, a record of an earlier run is
/// taken away, so `update/status` says no `harness` and the app shows no
/// step; with it on and Harness missing, `absent` until a round
/// reaches the network, unless a record already says where it is.
pub fn at_start(root: &Path, records: &Path, switch: bool, now_unix_ms: u64) {
    if !switch {
        for name in [RECORD, ASK] {
            let _ = fs::remove_file(records.join(name));
        }
        return;
    }
    if !present(root) && record(records).is_none() {
        write(records, state("absent", now_unix_ms));
    }
}

/// After a round that reached the channel, with the switch on: Harness
/// already there is recorded `installed` (put there before, or by hand),
/// and nothing runs; missing, with no try under way, `installing` is
/// recorded, the pin handed over in [`ASK`], and [`UNIT`] written and
/// started without waiting. A pin set wrongly, or a unit that cannot be
/// started, is recorded `failed`. What was recorded is returned, none when
/// nothing changed.
pub fn after_round(
    root: &Path,
    records: &Path,
    pin: &Result<Option<Pin>, String>,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Option<HarnessInstall> {
    let before = record(records);
    if present(root) {
        if before
            .as_ref()
            .is_some_and(|said| said.install == "installed")
        {
            return None;
        }
        let said = write(records, state("installed", now_unix_ms));
        say(&format!("Harness is at {HARNESS}: recorded as installed"));
        return Some(said);
    }
    if let Some(said) = &before
        && said.install == "installing"
        && now_unix_ms.saturating_sub(said.at_unix_ms) < STUCK_AFTER.as_millis() as u64
    {
        return None;
    }
    let pin = match pin {
        Ok(pin) => pin.clone(),
        Err(why) => {
            say(&format!("Harness is not installed: {why}"));
            return Some(write(records, failed(now_unix_ms, None, "pin_unreadable")));
        }
    };
    let pinned = Some(pin.is_some());
    let ask = Ask { version: 1, pin };
    let asked = serde_json::to_vec(&ask)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            write_whole(&records.join(ASK), &text, 0o600).map_err(|error| error.to_string())
        });
    let started = asked.and_then(|()| {
        write(
            records,
            HarnessInstall {
                pinned,
                ..state("installing", now_unix_ms)
            },
        );
        start(root, systemd)
    });
    match started {
        Ok(()) => {
            say(&format!(
                "Harness is not at {HARNESS}: {UNIT} started to put it there ({})",
                if ask.pin.is_some() {
                    "pinned"
                } else {
                    "Harness's own manifest, not pinned"
                }
            ));
            record(records)
        }
        Err(why) => {
            say(&format!(
                "Harness is not installed: {UNIT} not started: {why}"
            ));
            Some(write(records, failed(now_unix_ms, pinned, "not_started")))
        }
    }
}

/// [`TEXT`] written at [`UNIT`] unless it is there, systemd reloaded if
/// it was, what systemd loaded checked to be it, and the unit started
/// without waiting for its end.
fn start(root: &Path, systemd: &mut dyn Systemd) -> Result<(), String> {
    if crate::writer_unit::write_unit(root, UNIT, TEXT)? {
        systemd.reload()?;
    }
    crate::writer_unit::loaded_as(
        &systemd.harness_unit_properties()?,
        UNIT,
        "the Harness unit",
    )?;
    systemd.start_harness_unit()
}

/// `lamp-update harness-install`, run by [`UNIT`] as root: with Harness
/// missing, the pinned manifest (if [`ASK`] names one) fetched and its
/// sha256 checked, then Harness's own installer as `harness-on` runs it
/// ([`install::harness_on_pinned_with`]), `harness version` read, and
/// lamp-tasks started again so it runs with Harness. Harness already there
/// is recorded `installed` and nothing runs. Each step goes through `run`.
/// The record written is returned; an error only when another install
/// holds [`LOCK`], with nothing written.
pub fn install_now(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<HarnessInstall, String> {
    let _lock = lock(records)?;
    let before = record(records);
    if present(root) {
        let mut said = state("installed", now_unix_ms());
        said.version = version(run);
        said.pinned = before.and_then(|before| before.pinned);
        let said = write(records, said);
        say(&format!(
            "Harness is already at {HARNESS}: nothing installed"
        ));
        return Ok(said);
    }
    let ask = fs::read(records.join(ASK))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Ask>(&bytes).ok())
        .filter(|ask| ask.version == 1);
    let Some(ask) = ask else {
        say(&format!(
            "{ASK} is missing or unreadable: nothing installed"
        ));
        return finish(records, failed(now_unix_ms(), None, "pin_unreadable"));
    };
    let pinned = Some(ask.pin.is_some());
    let manifest = match &ask.pin {
        Some(pin) => match pinned_bytes(pin, run, say) {
            Ok(bytes) => Some(bytes),
            Err(error) => return finish(records, failed(now_unix_ms(), pinned, error)),
        },
        None => None,
    };
    // The checked bytes, served on 127.0.0.1 while the installer runs
    // (#5229): what it reads is what was checked.
    let served = match manifest.clone().map(serve::Served::start).transpose() {
        Ok(served) => served,
        Err(error) => {
            say(&format!(
                "the pinned Harness manifest was not served on 127.0.0.1: {error}"
            ));
            return finish(records, failed(now_unix_ms(), pinned, "installer"));
        }
    };
    let url = served.as_ref().map(serve::Served::url);
    let installed = install::harness_on_pinned_with(run, USER, HARNESS_HOME, url.as_deref());
    drop(served);
    if let Err(why) = installed {
        say(&format!("Harness is not installed: {why}"));
        return finish(records, failed(now_unix_ms(), pinned, reason_of(&why)));
    }
    let have = version(run);
    if let Some(want) = manifest.as_deref().and_then(product::version_of) {
        if have.as_deref() == Some(want.as_str()) {
            say(&format!("Harness {want} is the pinned version"));
        } else {
            say(&format!(
                "Harness says version {}, not the pinned {want}",
                have.as_deref().unwrap_or("none")
            ));
        }
    }
    let said = write(
        records,
        HarnessInstall {
            version: have,
            pinned,
            ..state("installed", now_unix_ms())
        },
    );
    say(&format!(
        "Harness {} installed for {USER} at {HARNESS}; not signed in, and its daemon not started",
        said.version
            .as_deref()
            .unwrap_or("of a version it doesn't say")
    ));
    // lamp-tasks waits for Harness (its `ConditionPathExists`): started now,
    // so it says why hand-overs are off and takes a sign-in.
    if let Err(why) = run(&["systemctl", "restart", "lamp-tasks.service"].map(str::to_owned)) {
        say(&format!("lamp-tasks.service not started again: {why}"));
    }
    Ok(said)
}

/// The pinned manifest's bytes, checked by its sha256: this build's own
/// copy, or the one at its https URL. A fetch that fails is `pin_fetch`;
/// other bytes are `pin_mismatch`, and nothing is installed.
fn pinned_bytes(
    pin: &Pin,
    run: &mut Runner,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<u8>, &'static str> {
    let bytes = if pin.built_in {
        product::MANIFEST.to_vec()
    } else {
        let fetched = run(&[
            "curl".to_owned(),
            "-fsSL".to_owned(),
            "--proto".to_owned(),
            "=https".to_owned(),
            "--tlsv1.2".to_owned(),
            "--max-filesize".to_owned(),
            MANIFEST_MOST_BYTES.to_owned(),
            pin.url.clone(),
        ]);
        match fetched {
            Ok(manifest) => manifest.into_bytes(),
            Err(why) => {
                say(&format!(
                    "the pinned Harness manifest was not fetched: {why}"
                ));
                return Err("pin_fetch");
            }
        }
    };
    let found = crate::manifest::sha256_hex(&bytes);
    if found != pin.sha256 {
        say(&format!(
            "the pinned Harness manifest's sha256 is {found}, not {}: nothing installed",
            pin.sha256
        ));
        return Err("pin_mismatch");
    }
    if pin.built_in {
        say(&format!(
            "the pinned Harness manifest is this build's own ({}, sha256 {found})",
            product::VERSION
        ));
    }
    Ok(bytes)
}

fn finish(records: &Path, said: HarnessInstall) -> Result<HarnessInstall, String> {
    Ok(write(records, said))
}

/// A failed install's fixed error code, from its error: past its bound, no
/// `lampos` user (the first step, `id -u`), else the installer's.
fn reason_of(why: &str) -> &'static str {
    if install::timed_out(why) {
        "timeout"
    } else if why.starts_with("id ") {
        "no_user"
    } else {
        "installer"
    }
}

/// `harness version`'s `a.b.c`, as `lampos` with Harness's home; none when
/// it cannot be read.
fn version(run: &mut Runner) -> Option<String> {
    let uid = install::uid_of(run, USER).ok()?;
    let text = run(&install::harness_argv(
        USER,
        HARNESS_HOME,
        &uid,
        &["version"],
    ))
    .ok()?;
    let (major, minor, patch) = install::harness_version_of(&text)?;
    Some(format!("{major}.{minor}.{patch}"))
}

/// [`LOCK`], held while this install runs; one held already is refused.
fn lock(records: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(records).map_err(|error| format!("{}: {error}", records.display()))?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(records.join(LOCK))
        .map_err(|error| format!("{LOCK}: {error}"))?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
        |_| io::Error::new(ErrorKind::WouldBlock, "another Harness install is at work").to_string(),
    )?;
    Ok(file)
}

pub mod product;
mod serve;

#[cfg(test)]
mod tests;
