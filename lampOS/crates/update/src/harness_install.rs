//! Harness put on a lamp before its sign-in (#5197; docs/install.md,
//! "Harness before the sign-in"), off unless `LAMP_HARNESS_AT_ONBOARDING=on`
//! in lamp-update's own unit; on a product lamp with it unset, the build's
//! own default ([`product::PRODUCT_DEFAULT`], on).
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
//! the next round, which the app's `update.check` starts at once; one that
//! would only fail the same way (`installer`, `no_user`) waits
//! [`RETRY_AFTER`] first.
//!
//! **Pinning (#3693's decision (a): a sha256 pin is enough):** with
//! `LAMP_HARNESS_MANIFEST_URL` and `LAMP_HARNESS_MANIFEST_SHA256` both set,
//! the manifest at that https URL is fetched and its sha256 compared first;
//! one that differs installs nothing (`pin_mismatch`). A match runs
//! Harness's installer with `HARNESS_METADATA_URL` at those checked bytes,
//! served on 127.0.0.1 while it runs ([`serve`], #5229), so the installer
//! checks what it fetches against the pinned manifest's sha256s and never
//! reads the manifest from its server again. The installer itself and the
//! four more manifests it reads are this build's copies
//! ([`product::INSTALLER`], [`product::MANIFESTS`]), each checked by its
//! sha256 too (`pin_mismatch`); the manifests are served from the same
//! listener and the installer is run from a file, never fetched. A
//! listener or file lamp-update cannot make is `serve`, tried again at
//! the next round. With neither set, the
//! install is as `harness-on`'s (Harness's own CDN manifest) and is said
//! `pinned: false`, except on a lamp installed as the product, which takes
//! the pin compiled into this build, covered by its signature, and never
//! installs unpinned ([`product`], #5229).
//!
//! **A newer pin moves an older Harness (#5957):** Harness there, recorded
//! `installed` at a version older than the pin's `cli.version`, is
//! installed again by the same one-shot, the same loopback kit and the
//! same hold, with Harness's home (its sign-in) kept, but only while
//! Harness is not in use ([`busy`]): a later round moves it otherwise, a
//! task worked on holding it back [`DEFER_CAP`] at most (#6656). The
//! Harness that was there is kept first ([`KEPT`]) and put back when the
//! new one fails, recorded `failed` with `upgrade` and `from_version`.
use crate::{
    install::{self, HARNESS, HARNESS_HOME, Runner, USER},
    records::write_whole,
    world::Systemd,
};
use lamp_msgs::HarnessInstall;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind, Write},
    net::TcpListener,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
/// A failure the same steps would only repeat (`installer`, `no_user`) is
/// tried again by the first round this long after it, not after every
/// round (#5241's reader); the rest, `serve` among them, are tried at the
/// next round.
pub const RETRY_AFTER: Duration = STUCK_AFTER;
/// A failed move to a newer pin (`upgrade`, #5957) is tried again with the
/// same pin only by a round this long after it; another pin at once.
pub const UPGRADE_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest a task on `tasks/status` holds back a move to a newer pin
/// (#6656, the first builder's (c), 13 Nov): a task left on a machine that
/// is offline is never seen to end, so past this the move goes ahead. A
/// sign-in or a hand-over running now still holds it back every round.
pub const DEFER_CAP: Duration = Duration::from_secs(6 * 60 * 60);
/// When a task on `tasks/status` first held back a move (#6656), in
/// lamp-update's records (0600): `{"version":1,"since_unix_ms":N}`. Gone
/// once nothing is in use, or once a move stops Harness.
pub const DEFERRED: &str = "harness-deferred.json";
/// Where a move keeps the Harness that was there, under Harness's home
/// (#5957), until it is installed or put back.
pub const KEPT: &str = ".harness/lampos-upgrade-from";
/// The largest pinned manifest fetched.
const MANIFEST_MOST_BYTES: &str = "1048576";

/// The one-shot unit. Fixed: a round passes no other name to systemd here.
pub const UNIT: &str = "lamp-update-harness.service";
/// The unit. Not `ProtectSystem=strict`: Harness's home
/// (`/var/lib/lampos-tasks`) may not be there yet, and is made under
/// `/var/lib`. `/etc` and `/usr` stay read-only but for `/usr/local/bin`,
/// where [`install::HARNESS`] is linked and Hermes's wrapper is written,
/// and `/usr/local/lib/lampos`, where Hermes's namespace script is
/// (#5978, [`crate::hermes_install`]; `-`: a lamp without that folder
/// still installs Harness). Its bound is past Harness's installer
/// ([`install::INSTALLER_WITHIN`]) and the steps around it, with Hermes
/// after it: an hour. `MemoryHigh=600M` and `CPUQuota=100%` (#5978): past
/// the memory cap the kernel reclaims the one-shot's own memory and
/// throttles it, never kills it. At 1200M and 150% the `products` stage took
/// `MemAvailable` down to about 786 MB on lamp-c0a4 and the voice lost its
/// privacy reading twice; at 600M and 100% it stayed above 1.4 GB and the
/// privacy reading held through the whole install.
pub const TEXT: &str = "\
# lamp-update-harness: puts Harness on the lamp before its sign-in, with Harness's own installer, installed only
# (lampOS/docs/install.md, \"Harness before the sign-in\", #5197). Written and started by a round of lamp-update
# with LAMP_HARNESS_AT_ONBOARDING=on, whose own sandbox cannot write /usr/local/bin or Harness's home; never
# enabled. Outside lampos.target, as lamp-update is. With Hermes on (LAMP_HERMES_ON_LAMP) it puts the pinned Hermes there
# after Harness, as lampos, with its wrapper in /usr/local/bin and its namespace script in /usr/local/lib/lampos
# (lampOS/docs/install.md, \"Hermes on the lamp\", #5978).
[Unit]
Description=lampOS updates, Harness put on the lamp (lamp-update-harness)
After=network-online.target

[Service]
Type=oneshot
User=root
Group=root
# The folder of the namespace scripts and the SDK is made before the sandbox, by \"+\" (no sandbox for this line): the
# \"-\" of ReadWritePaths skips a folder that is missing, and a lamp that took lamp-update from the channel never
# ran the installer that makes it (#6507).
ExecStartPre=+/usr/bin/install -d -m 0755 -o root -g root /usr/local/lib/lampos
ExecStart=/opt/lampOS/update/current/lamp-update harness-install
StandardInput=null
SyslogIdentifier=lamp-update-harness
TimeoutStartSec=3600
Nice=10
IOSchedulingClass=idle
CPUWeight=20
CPUQuota=100%
IOWriteBandwidthMax=/var/lib 8M
IOReadBandwidthMax=/var/lib 16M
IOWriteIOPSMax=/var/lib 200
IOReadIOPSMax=/var/lib 400
MemoryHigh=600M
NoNewPrivileges=yes
ProtectSystem=full
ReadWritePaths=/usr/local/bin -/usr/local/lib/lampos
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

/// What the round hands the one-shot in [`ASK`]. `upgrade_from` (#5957):
/// Harness is there at this older version, and is to be moved to the pin,
/// not left as "already at"; absent in an ask written before it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ask {
    version: u32,
    pin: Option<Pin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upgrade_from: Option<String>,
}

/// The ask [`ASK`] holds, when readable.
fn asked(records: &Path) -> Option<Ask> {
    fs::read(records.join(ASK))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Ask>(&bytes).ok())
        .filter(|ask| ask.version == 1)
}

/// Whether `have` is older than `want`, both dotted numbers compared part
/// by part (`0.5.100` is newer than `0.5.39`, a missing part is 0); none
/// when either is not plain digits and dots (#5957: never compared as
/// strings, and an unreadable version is never older).
pub fn older(have: &str, want: &str) -> Option<bool> {
    fn parts(version: &str) -> Option<Vec<u64>> {
        if version.is_empty() || version.len() > 32 {
            return None;
        }
        version
            .split('.')
            .map(|part| {
                (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| part.parse::<u64>().ok())
                    .flatten()
            })
            .collect()
    }
    let (have, want) = (parts(have)?, parts(want)?);
    let at = |list: &[u64], i: usize| list.get(i).copied().unwrap_or(0);
    let longest = have.len().max(want.len());
    for i in 0..longest {
        match at(&have, i).cmp(&at(&want, i)) {
            std::cmp::Ordering::Less => return Some(true),
            std::cmp::Ordering::Greater => return Some(false),
            std::cmp::Ordering::Equal => {}
        }
    }
    Some(false)
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

#[derive(Serialize, Deserialize)]
struct Deferred {
    version: u32,
    since_unix_ms: u64,
}

/// Whether `why` (Harness in use now, or none) still holds a move back at
/// `now_unix_ms` (#6656). None: nothing does, and the kept time is let
/// go. A sign-in or a hand-over running now always does. A task on
/// `tasks/status` ([`busy::is_task`]) does until [`DEFER_CAP`] after it
/// first did, that time kept in [`DEFERRED`] (written now when none can
/// be read); past it, one line says so and nothing holds the move back.
fn held_back(
    records: &Path,
    why: Option<&'static str>,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Option<&'static str> {
    let path = records.join(DEFERRED);
    let Some(why) = why else {
        let _ = fs::remove_file(&path);
        return None;
    };
    if !busy::is_task(why) {
        return Some(why);
    }
    let kept = fs::read(&path)
        .ok()
        .and_then(|text| serde_json::from_slice::<Deferred>(&text).ok())
        .filter(|kept| kept.version == 1 && kept.since_unix_ms <= now_unix_ms);
    let since = match kept {
        Some(kept) => kept.since_unix_ms,
        None => {
            let kept = Deferred {
                version: 1,
                since_unix_ms: now_unix_ms,
            };
            if let Ok(text) = serde_json::to_vec(&kept) {
                let _ = write_whole(&path, &text, 0o600);
            }
            now_unix_ms
        }
    };
    let waited = now_unix_ms - since;
    if waited < DEFER_CAP.as_millis() as u64 {
        return Some(why);
    }
    say(&format!(
        "Harness's move has waited {} h for {why}, past its {} h bound: it is moved now (#6656)",
        waited / 3_600_000,
        DEFER_CAP.as_secs() / 3600
    ));
    None
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

/// Harness at [`HARNESS`], its link followed: a link whose target is gone
/// is missing, so a round puts Harness there again (#5241's reader). An
/// absolute target is read under `root`, as the lamp's own `/` is.
pub fn present(root: &Path) -> bool {
    let place = root.join(HARNESS.trim_start_matches('/'));
    match fs::read_link(&place) {
        Ok(target) if target.is_absolute() => {
            fs::metadata(root.join(target.strip_prefix("/").unwrap_or(&target))).is_ok()
        }
        _ => fs::metadata(&place).is_ok(),
    }
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

/// [`after_round_when`], Harness's use read from the lamp's processes
/// under `root` only, no bus.
pub fn after_round(
    root: &Path,
    records: &Path,
    pin: &Result<Option<Pin>, String>,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Option<HarnessInstall> {
    after_round_when(
        root,
        records,
        pin,
        systemd,
        now_unix_ms,
        &mut || busy::in_use(root, None),
        say,
    )
}

/// After a round that reached the channel, with the switch on: Harness
/// already there is recorded `installed` (put there before, or by hand),
/// unless a pinned install recorded it `version`, and nothing runs; missing, with no try under way, `installing` is
/// recorded, the pin handed over in [`ASK`], and [`UNIT`] written and
/// started without waiting. A pin set wrongly, or a unit that cannot be
/// started, is recorded `failed`. Harness there at a version older than
/// the pin (#5957, [`upgrade_due`]) is moved by the same one-shot, unless
/// `in_use` says why not now: then one line is said and a later round
/// moves it. What was recorded is returned, none when nothing changed.
pub fn after_round_when(
    root: &Path,
    records: &Path,
    pin: &Result<Option<Pin>, String>,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    in_use: &mut dyn FnMut() -> Option<&'static str>,
    say: &mut dyn FnMut(&str),
) -> Option<HarnessInstall> {
    let before = record(records);
    if present(root) {
        if let (Some(said), Ok(Some(pin))) = (&before, pin)
            && let Some((from, want)) = upgrade_due(said, pin, records, now_unix_ms)
        {
            if let Some(why) = held_back(records, in_use(), now_unix_ms, say) {
                say(&format!(
                    "Harness {from} is to be moved to the pinned {want}: at a later round, {why}"
                ));
                return None;
            }
            return Some(ask_upgrade(
                root,
                records,
                pin,
                &from,
                systemd,
                now_unix_ms,
                say,
            ));
        }
        // A pinned install that left another version stays `version`
        // while Harness is there: it is not the pinned one. A move under
        // way, or one that failed and put the older one back, stays too.
        if before.as_ref().is_some_and(|said| {
            said.install == "installed"
                || matches!(said.error.as_deref(), Some("version" | "upgrade"))
                || (said.install == "installing"
                    && said.from_version.is_some()
                    && now_unix_ms.saturating_sub(said.at_unix_ms) < STUCK_AFTER.as_millis() as u64)
        }) {
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
    if let Some(said) = &before
        && said.install == "failed"
        && matches!(said.error.as_deref(), Some("installer" | "no_user"))
        && now_unix_ms.saturating_sub(said.at_unix_ms) < RETRY_AFTER.as_millis() as u64
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
    let ask = Ask {
        version: 1,
        pin,
        upgrade_from: None,
    };
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

/// Whether Harness, there as `said` records it, is to be moved to `pin`
/// (#5957): the version it is at and the pinned one (`newer` when only the
/// one-shot can know, an env pin's version being read once fetched).
///
/// - `installed` with a `version`; an `installing` move past
///   [`STUCK_AFTER`], from its `from_version`; a `failed` `upgrade`, from
///   its `from_version`, with another pin than [`ASK`]'s, or the same one
///   [`UPGRADE_RETRY_AFTER`] after it.
/// - The build's own pin: that version older than its `cli.version`
///   ([`older`]); an equal, newer or unreadable one is left.
/// - An env pin: a pin other than the one [`ASK`] handed over last; the
///   one-shot then compares the fetched `cli.version`.
fn upgrade_due(
    said: &HarnessInstall,
    pin: &Pin,
    records: &Path,
    now_unix_ms: u64,
) -> Option<(String, String)> {
    let since = now_unix_ms.saturating_sub(said.at_unix_ms);
    let last_pin = asked(records).and_then(|ask| ask.pin);
    let from = match (said.install.as_str(), said.error.as_deref()) {
        ("installed", _) => said.version.clone()?,
        ("installing", _) if since >= STUCK_AFTER.as_millis() as u64 => {
            said.from_version.clone()?
        }
        ("failed", Some("upgrade"))
            if last_pin.as_ref() != Some(pin)
                || since >= UPGRADE_RETRY_AFTER.as_millis() as u64 =>
        {
            said.from_version.clone()?
        }
        _ => return None,
    };
    if pin.built_in {
        let want = product::version_of(product::MANIFEST)?;
        return older(&from, &want)?.then_some((from, want));
    }
    // Plain digits and dots, as the one-shot will compare it. A move tried
    // before (stuck or failed, held above) is asked again with the same pin.
    older(&from, "0")?;
    (said.install != "installed" || last_pin.as_ref() != Some(pin))
        .then(|| (from, "version, read once fetched".to_owned()))
}

/// The move asked (#5957): [`ASK`] with `upgrade_from`, `installing` with
/// `from_version`, and [`UNIT`] started without waiting. A unit not
/// started is a `failed` `upgrade`, Harness left as it is.
fn ask_upgrade(
    root: &Path,
    records: &Path,
    pin: &Pin,
    from: &str,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> HarnessInstall {
    let ask = Ask {
        version: 1,
        pin: Some(pin.clone()),
        upgrade_from: Some(from.to_owned()),
    };
    let started = serde_json::to_vec(&ask)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            write_whole(&records.join(ASK), &text, 0o600).map_err(|error| error.to_string())
        })
        .and_then(|()| {
            write(records, HarnessInstall::upgrading(from, now_unix_ms));
            start(root, systemd)
        });
    match started {
        Ok(()) => {
            say(&format!(
                "Harness {from} is to be moved to the pin: {UNIT} started to move it (pinned)"
            ));
            record(records).unwrap_or_else(|| HarnessInstall::upgrading(from, now_unix_ms))
        }
        Err(why) => {
            say(&format!(
                "Harness {from} is not moved: {UNIT} not started: {why}"
            ));
            write(records, HarnessInstall::upgrade_failed(from, now_unix_ms))
        }
    }
}

/// [`TEXT`] written at [`UNIT`] unless it is there, systemd reloaded if
/// it was, what systemd loaded checked to be it, and the unit started
/// without waiting for its end.
pub(crate) fn start(root: &Path, systemd: &mut dyn Systemd) -> Result<(), String> {
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
/// lamp-tasks started again so it runs with Harness. Pinned, a `harness
/// version` other than the manifest's `cli.version` (or none) is recorded
/// `failed` with `version`, not `installed`. Pinned, the installer
/// is this build's checked copy, run from a file, with every manifest it
/// reads served checked from 127.0.0.1 ([`Kit`]). Harness already there
/// is moved when [`ASK`] asks it ([`upgrade`], which always ends
/// `installed` or `failed`, and the ask is then spent), else recorded
/// `installed` (a `failed` `version` or `upgrade` kept) and nothing runs.
/// Each step goes through `run`. The record written is returned; an error
/// only when another install holds [`LOCK`], with nothing written: a
/// move's `installing` then stays for that install to answer, or for the
/// round to ask again past [`STUCK_AFTER`].
pub fn install_now(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<HarnessInstall, String> {
    install_with(root, records, run, now_unix_ms, say, &Kit::BUILT)
}

/// What a pinned install runs and serves besides the CLI's manifest: this
/// build's own ([`Kit::BUILT`]); a test's may hold other bytes, or a
/// listener that cannot be made.
struct Kit {
    installer: &'static [u8],
    installer_sha256: &'static str,
    manifests: [product::Manifest; 4],
    bind: fn() -> io::Result<TcpListener>,
    /// Why Harness is in use now, looked at again just before a move
    /// stops it (#5957): [`busy::in_use_here`].
    in_use: fn(&Path) -> Option<&'static str>,
}

impl Kit {
    const BUILT: Kit = Kit {
        installer: product::INSTALLER,
        installer_sha256: product::INSTALLER_SHA256,
        manifests: product::MANIFESTS,
        bind: serve::loopback,
        in_use: busy::in_use_here,
    };

    /// Each file's bytes against its sha256, before anything runs: other
    /// bytes are `pin_mismatch`.
    fn check(&self, say: &mut dyn FnMut(&str)) -> Result<(), &'static str> {
        let installer =
            std::iter::once(("Harness's installer", self.installer, self.installer_sha256));
        let manifests = self
            .manifests
            .iter()
            .map(|manifest| (manifest.path, manifest.bytes, manifest.sha256));
        for (name, bytes, sha256) in installer.chain(manifests) {
            let found = crate::manifest::sha256_hex(bytes);
            if found != sha256 {
                say(&format!(
                    "the pinned {name}'s sha256 is {found}, not {sha256}: nothing installed"
                ));
                return Err("pin_mismatch");
            }
        }
        Ok(())
    }
}

/// The checked installer written as a file for `sh` to run: in a folder of
/// its own under the one-shot's private `/tmp` (`PrivateTmp=yes`), both
/// root's, the folder 0755 and the file 0644, so `lampos` reads it but
/// cannot change it. Taken away when dropped.
struct InstallerFile {
    folder: PathBuf,
}

impl InstallerFile {
    /// `bytes` written, then read back and checked against `sha256` just
    /// before the run: a write that cannot be made is `serve`, other bytes
    /// read back `pin_mismatch`.
    fn write(bytes: &[u8], sha256: &str, say: &mut dyn FnMut(&str)) -> Result<Self, &'static str> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let folder = std::env::temp_dir().join(format!(
            "lamp-update-harness-{}-{stamp}",
            std::process::id()
        ));
        let written = Self {
            folder: folder.clone(),
        };
        let made = fs::DirBuilder::new()
            .mode(0o755)
            .create(&folder)
            .and_then(|()| fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)))
            .and_then(|()| {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o644)
                    .open(written.path())?;
                file.write_all(bytes)?;
                file.sync_all()?;
                fs::set_permissions(written.path(), fs::Permissions::from_mode(0o644))
            });
        if let Err(error) = made {
            say(&format!(
                "the pinned Harness installer was not written at {}: {error}",
                folder.display()
            ));
            return Err("serve");
        }
        let found = fs::read(written.path()).map(|read| crate::manifest::sha256_hex(&read));
        if found.as_deref().ok() != Some(sha256) {
            say(&format!(
                "the pinned Harness installer written at {} is not sha256 {sha256}: nothing installed",
                folder.display()
            ));
            return Err("pin_mismatch");
        }
        Ok(written)
    }

    fn path(&self) -> PathBuf {
        self.folder.join(install::PINNED_INSTALLER)
    }
}

impl Drop for InstallerFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

/// A pinned install's pieces, held while its installer runs: the listener
/// serving the checked manifests, and the checked installer's file.
struct Handed {
    served: serve::Served,
    installer: InstallerFile,
    urls: Vec<(&'static str, String)>,
}

/// Every piece of a pinned install, checked and handed out: `manifest` (the
/// CLI's, checked already) and `kit`'s, each against its sha256
/// (`pin_mismatch`), served on 127.0.0.1, and the installer written as a
/// file (`serve` when either cannot be).
fn hand(manifest: Vec<u8>, kit: &Kit, say: &mut dyn FnMut(&str)) -> Result<Handed, &'static str> {
    kit.check(say)?;
    let mut files = vec![(serve::PATH.to_owned(), manifest)];
    files.extend(
        kit.manifests
            .iter()
            .map(|manifest| (manifest.path.to_owned(), manifest.bytes.to_vec())),
    );
    let served = (kit.bind)()
        .and_then(|listener| serve::Served::start(listener, files))
        .map_err(|error| {
            say(&format!(
                "the pinned Harness manifests were not served on 127.0.0.1: {error}"
            ));
            "serve"
        })?;
    let installer = InstallerFile::write(kit.installer, kit.installer_sha256, say)?;
    let mut urls = vec![("HARNESS_METADATA_URL", served.url(serve::PATH))];
    urls.extend(
        kit.manifests
            .iter()
            .map(|manifest| (manifest.env, served.url(manifest.path))),
    );
    Ok(Handed {
        served,
        installer,
        urls,
    })
}

fn install_with(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
) -> Result<HarnessInstall, String> {
    let _lock = lock(records)?;
    let before = record(records);
    if present(root) {
        // A move to a newer pin, asked by the round (#5957).
        if let Some(ask) = asked(records)
            && let (Some(from), Some(pin)) = (&ask.upgrade_from, &ask.pin)
        {
            let said = upgrade(root, records, run, now_unix_ms, say, kit, pin, from);
            // The move answered, `installed` or `failed` (#6275): the ask
            // keeps its pin without `upgrade_from`, so a one-shot started
            // later for Hermes moves nothing; a round asks again when
            // [`upgrade_due`] says so.
            let spent = Ask {
                version: 1,
                pin: Some(pin.clone()),
                upgrade_from: None,
            };
            if let Ok(text) = serde_json::to_vec(&spent) {
                let _ = write_whole(&records.join(ASK), &text, 0o600);
            }
            return Ok(said);
        }
        // A pinned install that left another version, or a move that
        // failed and put the older one back, stays as recorded, as a
        // round keeps it (`after_round_when`).
        if let Some(said) = before
            .as_ref()
            .filter(|said| matches!(said.error.as_deref(), Some("version" | "upgrade")))
        {
            say(&format!(
                "Harness is at {HARNESS}, recorded {}: nothing installed",
                said.error.as_deref().unwrap_or_default()
            ));
            return Ok(said.clone());
        }
        let mut said = state("installed", now_unix_ms());
        said.version = version(run);
        said.pinned = before.and_then(|before| before.pinned);
        let said = write(records, said);
        say(&format!(
            "Harness is already at {HARNESS}: nothing installed"
        ));
        return Ok(said);
    }
    let Some(ask) = asked(records) else {
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
    // The checked bytes, served on 127.0.0.1 while the installer runs, and
    // the checked installer run from a file (#5229): what runs and what it
    // reads is what was checked.
    let handed = match manifest
        .clone()
        .map(|bytes| hand(bytes, kit, say))
        .transpose()
    {
        Ok(handed) => handed,
        Err(error) => return finish(records, failed(now_unix_ms(), pinned, error)),
    };
    let installer = handed
        .as_ref()
        .map(|handed| handed.installer.path().to_string_lossy().into_owned());
    let pinned_run = handed
        .as_ref()
        .zip(installer.as_deref())
        .map(|(handed, installer)| install::Pinned {
            installer,
            urls: &handed.urls,
        });
    let installed = install::harness_on_pinned_with(run, USER, HARNESS_HOME, pinned_run.as_ref());
    // The listener stopped and the installer's file taken away once it ran.
    if let Some(handed) = handed {
        drop(handed.served);
        drop(handed.installer);
    }
    if let Err(why) = installed {
        say(&format!("Harness is not installed: {why}"));
        return finish(records, failed(now_unix_ms(), pinned, reason_of(&why)));
    }
    let have = version(run);
    let mut other_version = false;
    if let Some(want) = manifest.as_deref().and_then(product::version_of) {
        if have.as_deref() == Some(want.as_str()) {
            say(&format!("Harness {want} is the pinned version"));
        } else {
            say(&format!(
                "Harness says version {}, not the pinned {want}",
                have.as_deref().unwrap_or("none")
            ));
            other_version = true;
        }
    }
    let said = if other_version {
        // Installed, but not as pinned: `failed` with `version`, so the
        // app tells it from a pinned install. Kept while Harness is there
        // (`after_round`); installing again would only run the same steps.
        write(records, failed(now_unix_ms(), pinned, "version"))
    } else {
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
        said
    };
    // Pinned before lamp-tasks starts, when lamp-tasks holds Harness's
    // updates (#5493): a Harness command run by hand holds them too.
    launcher::after_install(root, run, say);
    // lamp-tasks waits for Harness (its `ConditionPathExists`): started now,
    // so it says why hand-overs are off and takes a sign-in.
    if let Err(why) = run(&["systemctl", "restart", "lamp-tasks.service"].map(str::to_owned)) {
        say(&format!("lamp-tasks.service not started again: {why}"));
    }
    Ok(said)
}

/// The Harness there kept at [`KEPT`], as `lampos` with Harness's home
/// (#5957): the folder of the `cli.js` its launcher runs (wherever this
/// Harness's installer put it), the launcher, the runtime's `current-*`
/// files and the `tmux` link. The runtimes themselves sit in folders named
/// by their version, which the installer never removes. Run as `lampos`,
/// so nothing in a folder `lampos` owns is written as root. A `cli.js`
/// folder that is, by its real path (`pwd -P`, so `/./` and `//` change
/// nothing), Harness's home, `.harness`, `.local`, `.local/bin`, the kept
/// copy, or a folder above one of them, is refused, so neither script can
/// remove the sign-in. A kept copy already there (a put-back that failed)
/// is the only good one: KEEP refuses it, and makes its own copy at
/// `<kept>.new`, renamed only once whole. Once kept, `current-node` is
/// taken away (#6302): Harness's installer reuses the Node it names, so a
/// moved Harness would stay on the older Node (22) while a fresh install
/// gets the pin's; without it, the installer takes the Node the pinned
/// runtime manifest names, checked by its sha256, and [`PUT_BACK`] puts
/// the kept one back when the move fails.
const KEEP: &str = r#"set -eu
k="$HOME/.harness/lampos-upgrade-from"
l="$HOME/.local/bin/harness"
H=$(cd "$HOME" && pwd -P) || exit 1
if [ -e "$k" ] || [ -L "$k" ]; then echo "a kept copy is already at $k: it is the only good one" >&2; exit 1; fi
cli=$(grep -o "$HOME/[^'\" ]*/cli\.js" "$l" | head -n 1)
[ -n "$cli" ] || exit 1
d=$(cd "$(dirname "$cli")" && pwd -P) || exit 1
for p in "$H" "$H/.harness" "$H/.local" "$H/.local/bin" "$H/.harness/lampos-upgrade-from"; do
  case "$p/" in "$d"/*) exit 1 ;; esac
done
case "$d" in "$H/.harness/lampos-upgrade-from"/* | "$H/.harness/lampos-upgrade-from.new"*) exit 1 ;; "$H"/*) ;; *) exit 1 ;; esac
n="$k.new"
rm -rf "$n"
mkdir -p "$n/runtime"
cp -a "$d" "$n/cli"
printf '%s\n' "$d" > "$n/cli-dir"
cp -a "$l" "$n/harness"
for f in current-node current-tmux current-grid; do
  if [ -e "$HOME/.harness/runtime/$f" ]; then cp -a "$HOME/.harness/runtime/$f" "$n/runtime/$f"; fi
done
if [ -L "$HOME/.local/bin/tmux" ]; then cp -P "$HOME/.local/bin/tmux" "$n/tmux"; fi
mv "$n" "$k"
rm -f "$HOME/.harness/runtime/current-node"
"#;
/// What [`KEEP`] kept, put back in place, as `lampos`. The folder it names
/// is checked again, by its real path, before anything is removed.
const PUT_BACK: &str = r#"set -eu
k="$HOME/.harness/lampos-upgrade-from"
H=$(cd "$HOME" && pwd -P) || exit 1
[ -d "$k/cli" ] || exit 1
[ -f "$k/harness" ] || exit 1
d=$(cat "$k/cli-dir")
b=$(basename "$d")
case "$b" in . | .. | "") exit 1 ;; esac
d="$(cd "$(dirname "$d")" && pwd -P)/$b" || exit 1
for p in "$H" "$H/.harness" "$H/.local" "$H/.local/bin" "$H/.harness/lampos-upgrade-from"; do
  case "$p/" in "$d"/*) exit 1 ;; esac
done
case "$d" in "$H/.harness/lampos-upgrade-from"/* | "$H/.harness/lampos-upgrade-from.new"*) exit 1 ;; "$H"/*) ;; *) exit 1 ;; esac
rm -rf "$d"
mkdir -p "$HOME/.local/bin" "$HOME/.harness/runtime"
cp -a "$k/cli" "$d"
cp -a "$k/harness" "$HOME/.local/bin/.harness.lampos-back"
mv -f "$HOME/.local/bin/.harness.lampos-back" "$HOME/.local/bin/harness"
for f in current-node current-tmux current-grid; do
  if [ -e "$k/runtime/$f" ]; then cp -a "$k/runtime/$f" "$HOME/.harness/runtime/$f"; fi
done
if [ -L "$k/tmux" ]; then ln -sfn "$(readlink "$k/tmux")" "$HOME/.local/bin/tmux"; fi
"#;
/// [`KEPT`] taken away once the move is done or put back.
const LET_GO: &str = r#"rm -rf "$HOME/.harness/lampos-upgrade-from""#;

/// `script` run by `sh` as `lampos`, with Harness's home as its `HOME`.
fn as_harness_user(script: &str) -> Vec<String> {
    vec![
        "runuser".to_owned(),
        "-u".to_owned(),
        USER.to_owned(),
        "--".to_owned(),
        "env".to_owned(),
        format!("HOME={HARNESS_HOME}"),
        "PATH=/usr/local/bin:/usr/bin:/bin".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
    ]
}

/// The one-shot's move of the Harness there, at `from` as the round read
/// it, to `pin` (#5957). `harness version` read again: none, or not older
/// than the pin's `cli.version`, is left as it is and recorded `installed`.
/// Else the pinned files checked as for a first install, Harness's use
/// looked at again (in use: `installed` as it was, moved at a later
/// round), lamp-tasks and then Harness's daemon stopped (either not
/// stopped: the same, lamp-tasks started again; lamp-tasks starts the new
/// daemon when it starts again), the Harness
/// there kept ([`KEEP`]), the installer run, and `harness version`
/// compared with the pin. A failed installer or another version puts the
/// kept one back ([`PUT_BACK`]) and records `failed` with `upgrade` and
/// `from_version`; so does a pin not checked or a copy not kept, with
/// nothing installed. Either way the launcher is pinned again (with the
/// hold on) and lamp-tasks started again.
#[allow(clippy::too_many_arguments)]
fn upgrade(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
    pin: &Pin,
    asked_from: &str,
) -> HarnessInstall {
    let Some(from) = version(run) else {
        say(&format!(
            "Harness at {HARNESS} says no version (asked to move it from {asked_from}): left as it is"
        ));
        return write(
            records,
            HarnessInstall {
                pinned: Some(true),
                ..state("installed", now_unix_ms())
            },
        );
    };
    // The version Harness says now, not the round's record (a lamp row on
    // #5957: recorded 0.5.39, Harness itself had moved to 0.5.62): the
    // `installing` record and any `failed` `upgrade` name it.
    if from != asked_from {
        say(&format!(
            "Harness says version {from}, not the recorded {asked_from}: moved from {from}"
        ));
        write(records, HarnessInstall::upgrading(&from, now_unix_ms()));
    }
    let left = |now: u64| HarnessInstall {
        version: Some(from.clone()),
        pinned: Some(true),
        ..state("installed", now)
    };
    let manifest = match pinned_bytes(pin, run, say) {
        Ok(bytes) => bytes,
        Err(_) => {
            say(&format!(
                "Harness {from} is not moved: the pin was not checked; it is kept as it is"
            ));
            return write(
                records,
                HarnessInstall::upgrade_failed(&from, now_unix_ms()),
            );
        }
    };
    let Some(want) = product::version_of(&manifest) else {
        say(&format!(
            "the pinned manifest names no plain version: Harness {from} is kept as it is"
        ));
        return write(records, left(now_unix_ms()));
    };
    if older(&from, &want) != Some(true) {
        say(&format!(
            "Harness {from} is not older than the pinned {want}: nothing installed"
        ));
        return write(records, left(now_unix_ms()));
    }
    let handed = match hand(manifest, kit, say) {
        Ok(handed) => handed,
        Err(_) => {
            say(&format!(
                "Harness {from} is not moved: the pinned files were not handed over; it is kept as it is"
            ));
            return write(
                records,
                HarnessInstall::upgrade_failed(&from, now_unix_ms()),
            );
        }
    };
    // Harness in use again just before it is stopped (#5957's pre-review):
    // the round's look may be minutes old. lamp-tasks is stopped after
    // this look, not before it, since stopping it would end a hand-over
    // it is sending.
    // A kept copy already there (a put-back that failed) is the only good
    // one: looked at before anything is stopped, and nothing is moved
    // until it is cleared by hand.
    let kept = root.join(HARNESS_HOME.trim_start_matches('/')).join(KEPT);
    if fs::symlink_metadata(&kept).is_ok() {
        drop(handed);
        say(&format!(
            "a kept copy waits at {HARNESS_HOME}/{KEPT}; Harness is not moved until it is cleared"
        ));
        return write(
            records,
            HarnessInstall::upgrade_failed(&from, now_unix_ms()),
        );
    }
    if let Some(why) = held_back(records, (kit.in_use)(root), now_unix_ms(), say) {
        drop(handed);
        say(&format!(
            "Harness {from} is to be moved to the pinned {want}: at a later round, {why}"
        ));
        return write(records, left(now_unix_ms()));
    }
    let stopped = install::uid_of(run, USER).and_then(|uid| {
        run(&["systemctl", "stop", "lamp-tasks.service"].map(str::to_owned))?;
        run(&install::harness_argv(USER, HARNESS_HOME, &uid, &["stop"]))
    });
    if let Err(why) = stopped {
        drop(handed);
        say(&format!(
            "Harness {from} is to be moved to the pinned {want}: at a later round, Harness was not stopped ({why})"
        ));
        // `start`, not `restart`: lamp-tasks may never have been stopped
        // (`id -u` failing first), and a running one is left as it is.
        if let Err(why) = run(&["systemctl", "start", "lamp-tasks.service"].map(str::to_owned)) {
            say(&format!("lamp-tasks.service not started again: {why}"));
        }
        return write(records, left(now_unix_ms()));
    }
    say(&format!("Harness {from} is moved to the pinned {want}"));
    // The move is under way: a later hold starts its bound afresh (#6656).
    let _ = fs::remove_file(records.join(DEFERRED));
    // A copy not kept is not let go: one already there (a put-back that
    // failed) is the only good one, and KEEP makes a new one beside it
    // (`.new`), renamed only once whole.
    let said = if let Err(why) = run(&as_harness_user(KEEP)) {
        drop(handed);
        say(&format!(
            "Harness {from} is not moved: it could not be kept to put back ({why}); nothing installed"
        ));
        HarnessInstall::upgrade_failed(&from, now_unix_ms())
    } else {
        let installer = handed.installer.path().to_string_lossy().into_owned();
        let pinned_run = install::Pinned {
            installer: &installer,
            urls: &handed.urls,
        };
        let installed = install::harness_on_pinned_with(run, USER, HARNESS_HOME, Some(&pinned_run));
        drop(handed);
        let have = version(run);
        match installed {
            Ok(()) if have.as_deref() == Some(want.as_str()) => {
                say(&format!("Harness moved from {from} to the pinned {want}"));
                let _ = run(&as_harness_user(LET_GO));
                HarnessInstall {
                    version: have,
                    pinned: Some(true),
                    ..state("installed", now_unix_ms())
                }
            }
            installed => {
                match installed {
                    Err(why) => say(&format!("Harness {want} not installed: {why}")),
                    Ok(()) => say(&format!(
                        "Harness says version {} after the move, not the pinned {want}",
                        have.as_deref().unwrap_or("none")
                    )),
                }
                put_back(run, &from, say);
                HarnessInstall::upgrade_failed(&from, now_unix_ms())
            }
        }
    };
    let said = write(records, said);
    launcher::after_install(root, run, say);
    if let Err(why) = run(&["systemctl", "restart", "lamp-tasks.service"].map(str::to_owned)) {
        say(&format!("lamp-tasks.service not started again: {why}"));
    }
    said
}

/// The kept Harness put back (#5957), its version said; [`KEPT`] taken
/// away only once `harness version` says `from` again.
fn put_back(run: &mut Runner, from: &str, say: &mut dyn FnMut(&str)) {
    let back = run(&as_harness_user(PUT_BACK));
    let ln = run(&[
        "ln".to_owned(),
        "-sfn".to_owned(),
        format!("{HARNESS_HOME}/.local/bin/harness"),
        HARNESS.to_owned(),
    ]);
    let now = version(run);
    match (back, ln, now.as_deref() == Some(from)) {
        (Ok(_), Ok(_), true) => {
            say(&format!("Harness {from} put back"));
            let _ = run(&as_harness_user(LET_GO));
        }
        (back, ln, _) => say(&format!(
            "Harness {from} not put back: Harness says version {} ({}); the kept copy stays at {HARNESS_HOME}/{KEPT}",
            now.as_deref().unwrap_or("none"),
            back.err()
                .or(ln.err())
                .unwrap_or_else(|| "another version".to_owned())
        )),
    }
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

pub mod busy;
pub mod launcher;
pub mod product;
mod serve;

#[cfg(test)]
mod tests;
