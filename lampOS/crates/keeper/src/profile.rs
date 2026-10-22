//! What a V1 lamp lacks for the generic lamp-os package, written by
//! `lamp-keeper prepare` (#2163): the package profile the package's units are
//! filled from, the settings and the runtime directories. Each is written
//! only when it is absent, so a lamp that has them (a V2 lamp, or one moved
//! by hand) keeps its own.
//!
//! The hardware is found by reading only: device links, sysfs and
//! `/proc/asound/cards`, and the mixer's control by asking for it. Nothing
//! is stopped and no hardware is moved, as prepare must (the parent
//! repository's `docs/bootstrap-ota.md`, "package adapter contract"). Where
//! a device is missing or there is more than one, it stops and says which,
//! and the old runtime keeps the lamp.
//!
//! The probe is `scripts/provision/lamp-v2-migrate`'s (#1980), which moved
//! lamp-a63e by hand.
use sha2::Digest;
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, chown},
    path::{Path, PathBuf},
};

/// The profile the package's adapter fills the units from.
pub const PROFILE: &str = "/etc/lampOS/package-profile.json";
/// This lamp's own poses, from lamp-body-measure (the factory's or by hand).
pub const LAMP_ENV: &str = "/etc/lampOS/body/lamp.env";
/// The parts' account: the V1 image's, which already has dialout, audio and video.
pub const LAMP_USER: &str = "orangepi";
/// Base yaw and head pitch counts that bring a face to the picture's middle:
/// the middle of two V1 lamps of this model (docs/body.md: 727,553 and
/// 782,557), until this lamp's is measured with a face.
pub const MODEL_GAZE: &str = "755,555";
/// The voice's microphone option for each V1 capture card it knows.
const MICROPHONES: [(&str, &str); 1] = [("device_micro2", "mic2")];
const SPEAKER: (&str, &str) = ("allwinnerhdmi", "Speaker");
const VOICE_OPTIONS: &str = "--noise-suppression off --turn-taking duplex --faint-floor-db -28 --microphone {mic} --reference-clock capture --reference-recovery bounded --provider-continuation kept-start --playout-start-ms 350 --end-silence-ms 600 --endpoint smart-turn-fast --call-wait-ms 2000";
const PARTS: [&str; 9] = [
    "voice",
    "eyes",
    "body",
    "policy",
    "name",
    "controls",
    "expression",
    "actions",
    "memory",
];
/// The generic controls run without `--touch`, which refuses these keys, and
/// without `LAMP_PRIVACY_CONTROLS=on`, which a `privacy` section needs: the
/// product's pins carry it since #2143, and lamp-controls refused to start
/// on a moved lamp-a63e with it (#2396's run, 21 Oct). So a moved lamp has
/// no `reset`, whatever its own files say (#2937, the first builder, 22:47):
/// V1's head pad idles high (`hal/drivers/ttp223.py`), as a button does, so
/// no idle read tells a pad on line 100 from the button, and a hand on the
/// head must never wipe a lamp. Its way back is #3020's hotspot.
const GENERIC_REFUSED_PINS: [&str; 3] = ["reset", "legacy_head", "privacy"];
/// Setup's own page and its reset plan, the ones `lamp-update install
/// --setup on` writes on a product lamp (#2937): setup's page and what a
/// reset would wipe. A moved lamp has no reset line (`GENERIC_REFUSED_PINS`),
/// so no hold acts on the plan until one is allowed. Not from the package's
/// `provision/`, so a package built before #2937 needs nothing new to carry
/// them.
const SETUP_SETTINGS: [(&str, &str, u32); 2] = [
    (
        "/etc/lampOS/setup/serve.json",
        include_str!("../../setup/fixtures/serve-own-page.json"),
        0o644,
    ),
    (
        SETUP_RESET_PLAN,
        include_str!("../../setup/fixtures/reset-plan-product.json"),
        0o644,
    ),
];
/// Where setup's reset plan goes: what a reset would wipe. A moved lamp has
/// no reset (`GENERIC_REFUSED_PINS`); the plan marks it prepared for setup.
const SETUP_RESET_PLAN: &str = "/etc/lampOS/setup/reset-plan.json";
/// Setup's marker, which lamp-setup writes once a lamp is set up and the
/// reset removes. A lamp moved from V1 is already on its home's Wi-Fi (it
/// got the package over it), so prepare writes the marker with the reset
/// plan, and its setup never opens a hotspot at the move. Only then: a lamp
/// with a plan and no marker had its marker removed (put back into setup),
/// and stays there.
pub const SETUP_MARKER: &str = "/var/lib/lampos-setup/done";
/// The settings the package carries in `provision/`, where each goes, and its mode.
const SETTINGS: [(&str, &str, u32); 3] = [
    (
        "voice-provider.json",
        "/etc/lampOS/voice/provider.json",
        0o600,
    ),
    ("controls.json", "/etc/lampOS/controls/controls.json", 0o644),
    (
        "controls-pins.json",
        "/etc/lampOS/controls/controls-pins.json",
        0o644,
    ),
];
/// The runtime directories: path, mode, and whether the parts' account owns it.
const DIRECTORIES: [(&str, u32, bool); 8] = [
    ("/var/lib/lampOS", 0o755, false),
    ("/var/lib/lampOS/voice", 0o700, false),
    ("/var/lib/lampOS/voice/sessions", 0o700, false),
    ("/var/lib/lampos-memory", 0o700, true),
    ("/var/lib/lampos-actions", 0o755, true),
    ("/var/lib/lampos-locks", 0o755, false),
    ("/var/lib/lampos-locks/motor", 0o700, true),
    ("/var/lib/lampos-locks/camera", 0o700, true),
];

/// What the probe found on this lamp.
#[derive(Debug, PartialEq, Eq)]
pub struct Hardware {
    pub servo: String,
    pub camera_device: String,
    pub camera_usb: String,
    pub microphone: String,
    pub mixer: (String, String),
    pub fast_cpus: String,
    pub judge_zones: String,
}

fn on(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn sorted_entries(folder: &Path, starts: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(starts))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The servo bus: the one CH340 adapter (`usb-1a86_*`) under `/dev/serial/by-id`.
pub fn servo_device(root: &Path) -> Result<String, String> {
    let links: Vec<PathBuf> = sorted_entries(&on(root, "/dev/serial/by-id"), "usb-1a86_")
        .into_iter()
        .filter(|path| path.is_symlink())
        .collect();
    let [link] = links.as_slice() else {
        return Err(format!(
            "expected one servo adapter (usb-1a86_*) under /dev/serial/by-id, found {}",
            links.len()
        ));
    };
    let target = fs::read_link(link).map_err(|error| format!("{}: {error}", link.display()))?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} points nowhere usable", link.display()))?;
    Ok(format!("/dev/{name}"))
}

/// The camera: the one USB video node with index 0, as its node and
/// `vendor:product@usb-path`.
pub fn camera(root: &Path) -> Result<(String, String), String> {
    let mut found = Vec::new();
    for node in sorted_entries(&on(root, "/sys/class/video4linux"), "video") {
        if read_trimmed(&node.join("index")).is_some_and(|index| index != "0") {
            continue;
        }
        let Ok(interface) = fs::canonicalize(node.join("device")) else {
            continue;
        };
        let Some(usb) = interface.parent() else {
            continue;
        };
        let (Some(vendor), Some(product)) = (
            read_trimmed(&usb.join("idVendor")),
            read_trimmed(&usb.join("idProduct")),
        ) else {
            continue;
        };
        let (Some(node_name), Some(usb_name)) = (
            node.file_name().and_then(|name| name.to_str()),
            usb.file_name().and_then(|name| name.to_str()),
        ) else {
            continue;
        };
        found.push((
            format!("/dev/{node_name}"),
            format!("{vendor}:{product}@{usb_name}"),
        ));
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        count => Err(format!("expected one USB camera, found {count}")),
    }
}

/// The ALSA card ids in `/proc/asound/cards` (` 0 [allwinnerhdmi ]: ...`).
fn card_ids(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(on(root, "/proc/asound/cards")).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line.strip_prefix(|c: char| c.is_ascii_digit())?;
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            let id = rest.trim_start().strip_prefix('[')?;
            Some(id.split([' ', ']']).next()?.to_owned())
        })
        .filter(|id| !id.is_empty())
        .collect()
}

/// The voice's microphone option, from the capture card this lamp has.
pub fn microphone(root: &Path) -> Result<String, String> {
    let cards = card_ids(root);
    MICROPHONES
        .iter()
        .find(|(card, _)| cards.iter().any(|id| id == card))
        .map(|(_, option)| (*option).to_owned())
        .ok_or_else(|| {
            let known: Vec<&str> = MICROPHONES.iter().map(|(card, _)| *card).collect();
            format!(
                "no microphone card this package knows ({}) in /proc/asound/cards",
                known.join(", ")
            )
        })
}

/// The speaker's card and control. `has_control` asks the mixer whether the
/// card has it (on a lamp, `amixer -c CARD sget CONTROL`, a read).
pub fn mixer(
    root: &Path,
    has_control: &mut dyn FnMut(&str, &str) -> bool,
) -> Result<(String, String), String> {
    let (card, control) = SPEAKER;
    if !card_ids(root).iter().any(|id| id == card) {
        return Err(format!(
            "the speaker's card {card} is not in /proc/asound/cards"
        ));
    }
    if !has_control(card, control) {
        return Err(format!("the mixer has no control {control} on {card}"));
    }
    Ok((card.to_owned(), control.to_owned()))
}

/// The CPUs with the highest top frequency, for the voice's affinity.
pub fn fast_cpus(root: &Path) -> Result<String, String> {
    let mut speeds = Vec::new();
    for cpu in sorted_entries(&on(root, "/sys/devices/system/cpu"), "cpu") {
        let Some(number) = cpu
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("cpu"))
            .and_then(|digits| digits.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(top) = read_trimmed(&cpu.join("cpufreq/cpuinfo_max_freq"))
            .and_then(|text| text.parse::<u64>().ok())
        {
            speeds.push((number, top));
        }
    }
    let Some(fastest) = speeds.iter().map(|(_, top)| *top).max() else {
        return Err("no CPU frequencies under /sys/devices/system/cpu".into());
    };
    let mut fast: Vec<u32> = speeds
        .iter()
        .filter(|(_, top)| *top == fastest)
        .map(|(number, _)| *number)
        .collect();
    fast.sort_unstable();
    Ok(fast
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" "))
}

/// The thermal zones the health judges by: `cpu` and `skin`, those there.
pub fn judge_zones(root: &Path) -> Result<String, String> {
    let types: Vec<String> = sorted_entries(&on(root, "/sys/class/thermal"), "thermal_zone")
        .iter()
        .filter_map(|zone| read_trimmed(&zone.join("type")))
        .collect();
    let zones: Vec<String> = ["cpu", "skin"]
        .iter()
        .filter(|zone| types.iter().any(|kind| kind.starts_with(*zone)))
        .map(|zone| format!("--judge-zone {zone}"))
        .collect();
    if zones.is_empty() {
        return Err("no cpu or skin thermal zone under /sys/class/thermal".into());
    }
    Ok(zones.join(" "))
}

/// Everything the profile needs from the hardware, or the first thing missing.
pub fn probe(
    root: &Path,
    has_control: &mut dyn FnMut(&str, &str) -> bool,
) -> Result<Hardware, String> {
    let (camera_device, camera_usb) = camera(root)?;
    Ok(Hardware {
        servo: servo_device(root)?,
        camera_device,
        camera_usb,
        microphone: microphone(root)?,
        mixer: mixer(root, has_control)?,
        fast_cpus: fast_cpus(root)?,
        judge_zones: judge_zones(root)?,
    })
}

/// `BODY_POSES` from a lamp.env, with the model's gaze where it has none.
pub fn body_poses(lamp_env: &str) -> Result<String, String> {
    let poses = lamp_env
        .lines()
        .find_map(|line| {
            line.trim_end()
                .strip_prefix("BODY_POSES=\"")?
                .strip_suffix('"')
        })
        .filter(|poses| poses.contains("--home "))
        .ok_or("lamp.env has no BODY_POSES with --home")?;
    let mut poses = poses.to_owned();
    if !poses.contains("--gaze ") {
        poses.push_str(" --gaze ");
        poses.push_str(MODEL_GAZE);
    }
    if poses.contains(['%', '$', '\\', '"', '\'']) || poses.chars().any(char::is_control) {
        return Err("BODY_POSES holds a character a unit cannot carry".into());
    }
    Ok(poses)
}

/// The profile for this lamp, as the adapter reads it. `BODY_POSES` only
/// when the lamp has its `lamp.env` already: the generic body unit reads its
/// poses from that file, or from the seated ones `lamp-body-measure.service`
/// reads after cutover (#2163); the substitution is for older packages'
/// units, which took them in the unit.
pub fn profile(unit_id: &str, hardware: &Hardware, poses: Option<&str>) -> serde_json::Value {
    let health_expect: Vec<String> = PARTS
        .iter()
        .map(|part| format!("--expect {part}"))
        .collect();
    let gaze = match poses {
        Some(poses) if !poses_gaze_is_model(poses) => "measured",
        Some(_) => "default, not measured",
        None => "not measured",
    };
    let mut profile = serde_json::json!({
        "unit_id": unit_id,
        "gaze": gaze,
        "substitutions": {
            "UNIT_ID": unit_id,
            "LAMP_USER": LAMP_USER,
            "MEMORY_USER": LAMP_USER,
            "SERVO_DEVICE": hardware.servo,
            "CAMERA_DEVICE": hardware.camera_device,
            "CAMERA_USB": hardware.camera_usb,
            "MOTOR_LOCK_DIR": "/var/lib/lampos-locks/motor",
            "CAMERA_LOCK_DIR": "/var/lib/lampos-locks/camera",
            "VOICE_OPTIONS": VOICE_OPTIONS.replace("{mic}", &hardware.microphone),
            "FAST_CPUS": hardware.fast_cpus,
            "MEMORY_DIR": "/var/lib/lampos-memory",
            "MEMORY_SOCKET": "/var/lib/lampos-memory/memory.sock",
            "ACTIONS_DIR": "/var/lib/lampos-actions",
            "MIXER_CARD": hardware.mixer.0,
            "MIXER_CONTROL": hardware.mixer.1,
            "HEALTH_EXPECT": health_expect.join(" "),
            "JUDGE_ZONES": hardware.judge_zones,
        }
    });
    if let Some(poses) = poses {
        profile["substitutions"]["BODY_POSES"] = poses.into();
    }
    profile
}

fn poses_gaze_is_model(poses: &str) -> bool {
    poses.ends_with(&format!("--gaze {MODEL_GAZE}"))
}

/// The pins file without the keys the generic controls refuse: `--touch`'s
/// and the privacy switch's.
pub fn generic_pins(text: &str) -> Result<String, String> {
    let mut pins: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(text).map_err(|error| format!("controls-pins.json: {error}"))?;
    for key in GENERIC_REFUSED_PINS {
        pins.remove(key);
    }
    serde_json::to_string_pretty(&pins)
        .map(|text| text + "\n")
        .map_err(|error| error.to_string())
}

/// The account's uid and gid from `etc/passwd` under `root`.
fn account(root: &Path, name: &str) -> Result<(u32, u32), String> {
    let passwd = fs::read_to_string(on(root, "/etc/passwd"))
        .map_err(|error| format!("/etc/passwd: {error}"))?;
    passwd
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            (fields.first() == Some(&name))
                .then(|| Some((fields.get(2)?.parse().ok()?, fields.get(3)?.parse().ok()?)))?
        })
        .ok_or_else(|| format!("the account {name} is not in /etc/passwd"))
}

fn absent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err()
}

/// Write a new file whole, with its mode, by a temporary beside it linked
/// into place: a link never replaces a file that appeared meanwhile.
fn write_new(path: &Path, content: &str, mode: u32) -> Result<(), String> {
    let say = |error: std::io::Error| format!("{}: {error}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(say)?;
    }
    let temporary = path.with_extension("keeper-new");
    let mut file = fs::File::create(&temporary).map_err(say)?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(say)?;
    file.write_all(content.as_bytes()).map_err(say)?;
    file.sync_all().map_err(say)?;
    let linked = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    linked.map_err(say)
}

/// Write a file over the one there, whole, with its mode, by a temporary
/// beside it renamed into place: one written over is never seen half done.
fn write_over(path: &Path, content: &str, mode: u32) -> Result<(), String> {
    let say = |error: std::io::Error| format!("{}: {error}", path.display());
    let temporary = path.with_extension("keeper-new");
    let mut file = fs::File::create(&temporary).map_err(say)?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(say)?;
    file.write_all(content.as_bytes()).map_err(say)?;
    file.sync_all().map_err(say)?;
    fs::rename(&temporary, path).map_err(say)
}

/// What `prepare` writes: what is missing of the profile, the settings and
/// the directories, under `root` (`/` on a lamp), from the package at
/// `release`. Returns the paths written. A lamp with a profile is left as it
/// is; one without stops at the first device or pose it cannot find, before
/// any of these is written.
pub fn prepare(
    root: &Path,
    release: &Path,
    has_control: &mut dyn FnMut(&str, &str) -> bool,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    prepare_with(root, release, &[], has_control, say)
}

/// [`prepare`], and a reset plan an earlier build shipped written over with
/// this build's (El on #2975): `shipped_plans` are the SHA-256 of every plan
/// a build carried (`lamp_update::install::RESET_PLAN_SHIPPED`). A moved lamp
/// keeps its first plan otherwise, so a store added later (#2947's face
/// consent) would survive a reset, once one is allowed. A plan changed by
/// hand is kept.
pub fn prepare_with(
    root: &Path,
    release: &Path,
    shipped_plans: &[&str],
    has_control: &mut dyn FnMut(&str, &str) -> bool,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    let mut wrote = Vec::new();
    let profile_path = on(root, PROFILE);
    // Everything is read and checked before the first write, so a refusal
    // leaves the lamp as it was.
    let profile_text = if absent(&profile_path) {
        let unit_id = read_trimmed(&on(root, "/etc/hostname"))
            .filter(|name| !name.is_empty())
            .ok_or("/etc/hostname is empty or missing")?;
        let hardware = probe(root, has_control)?;
        say(&format!(
            "{unit_id}: servo {}, camera {} ({}), microphone {}, speaker {}/{}, fast CPUs {}, {}",
            hardware.servo,
            hardware.camera_device,
            hardware.camera_usb,
            hardware.microphone,
            hardware.mixer.0,
            hardware.mixer.1,
            hardware.fast_cpus,
            hardware.judge_zones
        ));
        // No poses yet is not a refusal: lamp-body-measure.service reads
        // them seated, at rest, after cutover, and the body stays seated
        // until a person confirms its first stand (#2163).
        let poses = match fs::read_to_string(on(root, LAMP_ENV)) {
            Ok(lamp_env) => Some(body_poses(&lamp_env)?),
            Err(_) => {
                say(&format!(
                    "no {LAMP_ENV}: the poses are read seated after cutover (lamp-body-measure.service)"
                ));
                None
            }
        };
        if poses.as_deref().is_some_and(poses_gaze_is_model) {
            say(&format!(
                "gaze: the model's {MODEL_GAZE}, not measured on this lamp"
            ));
        }
        let text = serde_json::to_string_pretty(&profile(&unit_id, &hardware, poses.as_deref()))
            .map_err(|error| error.to_string())?;
        Some(text + "\n")
    } else {
        None
    };
    let mut settings = Vec::new();
    for (name, target, mode) in SETTINGS {
        if !absent(&on(root, target)) {
            continue;
        }
        let source = release.join("provision").join(name);
        let text = fs::read_to_string(&source)
            .map_err(|error| format!("the package has no {}: {error}", source.display()))?;
        let text = if name == "controls-pins.json" {
            generic_pins(&text)?
        } else {
            text
        };
        settings.push((target, text, mode));
    }
    let marker = absent(&on(root, SETUP_RESET_PLAN)) && absent(&on(root, SETUP_MARKER));
    let mut plan_over = None;
    for (target, text, mode) in SETUP_SETTINGS {
        if absent(&on(root, target)) {
            settings.push((target, text.to_owned(), mode));
        } else if target == SETUP_RESET_PLAN {
            let kept = fs::read(on(root, target)).map_err(|error| format!("{target}: {error}"))?;
            let sum: String = sha2::Sha256::digest(&kept)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if kept != text.as_bytes() && shipped_plans.contains(&sum.as_str()) {
                plan_over = Some((text, mode));
            }
        }
    }
    // The account only when a directory it owns is missing: a lamp with
    // them all is never refused for its accounts (the reviewer on #2170).
    let owner = if DIRECTORIES
        .iter()
        .any(|(path, _, owned)| *owned && absent(&on(root, path)))
    {
        Some(account(root, LAMP_USER)?)
    } else {
        None
    };

    if let Some(text) = profile_text {
        write_new(&profile_path, &text, 0o644)?;
        wrote.push(PROFILE.to_owned());
    }
    // The marker before the plan (the Reviewer-lead on #2975): a power cut
    // between the two leaves a marker and no plan, which the next prepare
    // completes, never a plan and no marker, which reads as a lamp reset
    // into setup and would open its hotspot by itself.
    if marker {
        // As lamp-setup keeps it: its state directory root's only (the
        // unit's `StateDirectoryMode=0700`), the marker empty.
        let folder = on(root, "/var/lib/lampos-setup");
        let failed = |error: std::io::Error| format!("/var/lib/lampos-setup: {error}");
        if absent(&folder) {
            fs::create_dir_all(&folder).map_err(failed)?;
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).map_err(failed)?;
        }
        write_new(&on(root, SETUP_MARKER), "", 0o600)?;
        wrote.push(SETUP_MARKER.to_owned());
    }
    for (target, text, mode) in settings {
        write_new(&on(root, target), &text, mode)?;
        wrote.push(target.to_owned());
    }
    if let Some((text, mode)) = plan_over {
        write_over(&on(root, SETUP_RESET_PLAN), text, mode)?;
        wrote.push(SETUP_RESET_PLAN.to_owned());
        say(&format!(
            "{SETUP_RESET_PLAN} was a plan an earlier build shipped: written over with this build's"
        ));
    }
    for (path, mode, owned) in DIRECTORIES {
        let folder = on(root, path);
        if !absent(&folder) {
            continue;
        }
        let failed = |error: std::io::Error| format!("{path}: {error}");
        fs::create_dir_all(&folder).map_err(failed)?;
        fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).map_err(failed)?;
        if let (true, Some((uid, gid))) = (owned, owner) {
            chown(&folder, Some(uid), Some(gid)).map_err(failed)?;
        }
        wrote.push(path.to_owned());
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};

    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "lamp-keeper-profile-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, path: &str, text: &str) {
            let path = on(&self.0, path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn link(&self, path: &str, target: &str) {
            let path = on(&self.0, path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(target, path).unwrap();
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const CARDS: &str = " 0 [allwinnerhdmi ]: allwinner-hdmi - allwinner-hdmi\n                      allwinner-hdmi\n 1 [device_micro2  ]: USB-Audio - device_micro2\n                      device_micro2 at usb-xhci\n";
    const LAMP_ENV_TEXT: &str = "# lamp-a63e, read at rest\nBODY_POSES=\"--home 1768,2060,2036 --folded-arm 2048,2046 --stand 1740,2060,2036,1300,1200 --pitch-window 1740,2030 --torque on\"\n";

    /// A V1 lamp as lamp-a63e is: one servo adapter, one USB camera (and
    /// its metadata node), the two sound cards, four slow and two fast CPUs.
    fn v1_lamp() -> Dir {
        let dir = Dir::new();
        dir.file("/etc/hostname", "lamp-a63e\n");
        // The parts' account is this test's own, so the chown needs no root.
        let mine = fs::metadata(&dir.0).unwrap();
        let me = format!(
            "{LAMP_USER}:x:{}:{}::/home/{LAMP_USER}:/bin/bash\n",
            mine.uid(),
            mine.gid()
        );
        dir.file("/etc/passwd", &format!("root:x:0:0::/root:/bin/bash\n{me}"));
        dir.file("/dev/ttyACM0", "");
        dir.link(
            "/dev/serial/by-id/usb-1a86_USB_Single_Serial_5A46-if00",
            "../../ttyACM0",
        );
        let usb = "/sys/devices/platform/usb/3-1/3-1.1/3-1.1.4";
        dir.file(&format!("{usb}/idVendor"), "01da\n");
        dir.file(&format!("{usb}/idProduct"), "5875\n");
        for (node, index) in [("video0", "0"), ("video1", "1")] {
            dir.file(&format!("{usb}/3-1.1.4:1.0/video4linux/{node}/x"), "");
            dir.file(&format!("/sys/class/video4linux/{node}/index"), index);
            dir.link(
                &format!("/sys/class/video4linux/{node}/device"),
                &on(&dir.0, &format!("{usb}/3-1.1.4:1.0"))
                    .display()
                    .to_string(),
            );
        }
        dir.file("/proc/asound/cards", CARDS);
        for (cpu, top) in [
            (0, 1_416_000),
            (1, 1_416_000),
            (4, 2_208_000),
            (5, 2_208_000),
        ] {
            dir.file(
                &format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq"),
                &format!("{top}\n"),
            );
        }
        dir.file("/sys/devices/system/cpu/cpufreq/x", "");
        dir.file("/sys/class/thermal/thermal_zone0/type", "cpu-thermal\n");
        dir.file("/sys/class/thermal/thermal_zone1/type", "gpu-thermal\n");
        dir.file(LAMP_ENV, LAMP_ENV_TEXT);
        dir.file(
            "/release/provision/voice-provider.json",
            "{\"provider\":\"second\"}\n",
        );
        dir.file("/release/provision/controls.json", "{\"touch\":false}\n");
        dir.file(
            "/release/provision/controls-pins.json",
            "{\"button\":12,\"reset\":7,\"legacy_head\":3,\"privacy\":{\"line\":9}}\n",
        );
        // Its own wiring as V1 keeps it since 18 Sep: the reset button on
        // line 100, the pads elsewhere (the parent repository's
        // robots/lamp/gpio_button.json and ttp223.json).
        dir.file("/opt/devices/lamp/gpio_button.json", V1_BUTTONS);
        dir.file(
            "/opt/devices/lamp/ttp223.json",
            "{\"boards\":{\"orangepi_sun60\":{\"chip\":0,\"lines\":[37,96,97,98],\"detect\":true}}}\n",
        );
        dir
    }

    /// The parent repository's `robots/lamp/gpio_button.json` since
    /// `8b5c343e1` (18 Sep), the Orange Pi's part.
    const V1_BUTTONS: &str = r#"{"boards":{"orangepi_sun60":{"buttons":[
        {"name":"primary","chip":0,"line":99,"debounce_ns":200000000,"behavior":"standard","factory_reset":false},
        {"name":"factory_reset","chip":0,"line":100,"debounce_ns":200000000,"behavior":"factory_reset","hold_s":5}]}}}"#;

    /// Where the generic controls read their pins.
    const PINS: &str = "/etc/lampOS/controls/controls-pins.json";

    fn pins_of(lamp: &Dir) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(on(&lamp.0, PINS)).unwrap()).unwrap()
    }

    /// #2937 (the first builder, 22:47 on #2975, after El's P1): a lamp
    /// moved from V1 gets no `reset`, whatever its own wiring says: V1's
    /// head pad idles high, as a button does, so a pad on line 100 can't be
    /// told from the button. With a button file (V1's since 18 Sep), a pad
    /// file naming line 100, and no files at all, the pins carry no `reset`,
    /// nor `legacy_head` or the slide; the rest is the product's.
    #[test]
    fn a_moved_lamp_gets_no_reset_whatever_its_wiring_says() {
        let pad_on_100 = "{\"boards\":{\"orangepi_sun60\":{\"chip\":0,\"lines\":[96,100]}}}";
        for case in ["button", "pad", "none"] {
            let lamp = v1_lamp();
            match case {
                "pad" => lamp.file("/opt/devices/lamp/ttp223.json", pad_on_100),
                "none" => fs::remove_dir_all(on(&lamp.0, "/opt/devices/lamp")).unwrap(),
                _ => {}
            }
            prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
            let pins = pins_of(&lamp);
            for refused in ["reset", "legacy_head", "privacy"] {
                assert!(pins.get(refused).is_none(), "{case}: {refused} in {pins}");
            }
            assert_eq!(pins["button"], 12, "{case}");
        }
    }

    fn yes(_: &str, _: &str) -> bool {
        true
    }

    /// Writes the pins file prepare gives a lamp moved from V1, from this
    /// build's product file, as a kept store for lamp-controls' tests
    /// (`crates/controls/tests/fixtures/stores/README.md`), when
    /// `LAMP_CONTROLS_MOVED_PINS_FIXTURE` names the file to write.
    #[test]
    #[ignore = "writes a fixture"]
    fn write_a_moved_pins_fixture() {
        let target = std::env::var_os("LAMP_CONTROLS_MOVED_PINS_FIXTURE")
            .expect("LAMP_CONTROLS_MOVED_PINS_FIXTURE");
        let product = include_str!("../../../deploy/product/settings/controls-pins.json");
        std::fs::write(target, generic_pins(product).unwrap()).unwrap();
    }

    /// #2529: the generic voice unit carries the ring's ceiling itself, and
    /// the voice refuses the flag given twice, so the profile's voice
    /// options never carry it.
    #[test]
    fn the_voice_options_never_carry_the_ring_ceiling() {
        assert!(!VOICE_OPTIONS.contains("--ring-channel-ceiling"));
    }

    #[test]
    fn the_probe_finds_a_v1_lamps_hardware() {
        let lamp = v1_lamp();
        let hardware = probe(&lamp.0, &mut yes).unwrap();
        assert_eq!(
            hardware,
            Hardware {
                servo: "/dev/ttyACM0".into(),
                camera_device: "/dev/video0".into(),
                camera_usb: "01da:5875@3-1.1.4".into(),
                microphone: "mic2".into(),
                mixer: ("allwinnerhdmi".into(), "Speaker".into()),
                fast_cpus: "4 5".into(),
                judge_zones: "--judge-zone cpu".into(),
            }
        );
    }

    #[test]
    fn the_probe_stops_on_a_missing_or_doubled_device() {
        let lamp = v1_lamp();
        lamp.link(
            "/dev/serial/by-id/usb-1a86_USB_Single_Serial_9999-if00",
            "../../ttyACM1",
        );
        assert!(servo_device(&lamp.0).unwrap_err().contains("found 2"));

        let lamp = v1_lamp();
        fs::remove_dir_all(on(&lamp.0, "/dev/serial")).unwrap();
        assert!(servo_device(&lamp.0).unwrap_err().contains("found 0"));

        let lamp = v1_lamp();
        lamp.file("/proc/asound/cards", " 0 [allwinnerhdmi ]: x\n");
        assert!(microphone(&lamp.0).unwrap_err().contains("device_micro2"));
        assert!(
            mixer(&lamp.0, &mut |_, _| false)
                .unwrap_err()
                .contains("Speaker")
        );

        let lamp = v1_lamp();
        fs::remove_dir_all(on(&lamp.0, "/sys/class/thermal")).unwrap();
        assert!(judge_zones(&lamp.0).is_err());
    }

    #[test]
    fn a_second_camera_is_refused_not_guessed() {
        let lamp = v1_lamp();
        let usb = "/sys/devices/platform/usb/3-1/3-1.2";
        lamp.file(&format!("{usb}/idVendor"), "046d\n");
        lamp.file(&format!("{usb}/idProduct"), "0825\n");
        lamp.file(&format!("{usb}/3-1.2:1.0/x"), "");
        lamp.file("/sys/class/video4linux/video2/index", "0");
        lamp.link(
            "/sys/class/video4linux/video2/device",
            &on(&lamp.0, &format!("{usb}/3-1.2:1.0"))
                .display()
                .to_string(),
        );
        assert_eq!(
            camera(&lamp.0).unwrap_err(),
            "expected one USB camera, found 2"
        );
    }

    #[test]
    fn poses_take_the_models_gaze_until_this_lamps_is_measured() {
        let poses = body_poses(LAMP_ENV_TEXT).unwrap();
        assert!(poses.starts_with("--home 1768,2060,2036 "));
        assert!(poses.ends_with("--gaze 755,555"));
        let measured = body_poses("BODY_POSES=\"--home 1,2,3 --gaze 727,553\"\n").unwrap();
        assert_eq!(measured, "--home 1,2,3 --gaze 727,553");
        assert!(body_poses("BODY_POSES=\"--folded-arm 1,2\"\n").is_err());
        assert!(body_poses("BODY_POSES=\"--home 1,2,3 $X\"\n").is_err());
    }

    #[test]
    fn prepare_writes_what_is_missing_and_says_the_gaze_is_the_models() {
        let lamp = v1_lamp();
        let mut said = Vec::new();
        let wrote = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |line| {
            said.push(line.to_owned())
        })
        .unwrap();
        assert_eq!(wrote[0], PROFILE);
        assert!(wrote.contains(&"/etc/lampOS/controls/controls-pins.json".to_owned()));
        assert!(wrote.contains(&"/var/lib/lampos-locks/motor".to_owned()));
        assert!(said.iter().any(|line| line.contains("not measured")));

        let profile: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(on(&lamp.0, PROFILE)).unwrap()).unwrap();
        assert_eq!(profile["unit_id"], "lamp-a63e");
        assert_eq!(profile["gaze"], "default, not measured");
        let substitutions = &profile["substitutions"];
        assert_eq!(substitutions["SERVO_DEVICE"], "/dev/ttyACM0");
        assert_eq!(substitutions["CAMERA_USB"], "01da:5875@3-1.1.4");
        assert!(
            substitutions["VOICE_OPTIONS"]
                .as_str()
                .unwrap()
                .contains("--microphone mic2")
        );
        assert!(
            substitutions["BODY_POSES"]
                .as_str()
                .unwrap()
                .ends_with("--gaze 755,555")
        );
        let mode = |path: &str| {
            fs::metadata(on(&lamp.0, path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(PROFILE), 0o644);
        assert_eq!(mode("/etc/lampOS/voice/provider.json"), 0o600);
        assert_eq!(mode("/var/lib/lampos-locks/motor"), 0o700);
        let pins =
            fs::read_to_string(on(&lamp.0, "/etc/lampOS/controls/controls-pins.json")).unwrap();
        assert!(
            pins.contains("button")
                && !pins.contains("reset")
                && !pins.contains("legacy_head")
                && !pins.contains("privacy"),
            "{pins}"
        );
        // #2937: setup's page and reset plan as install --setup on writes
        // them, and the marker, so the moved lamp is set up.
        for (path, text, _) in SETUP_SETTINGS {
            assert!(wrote.contains(&path.to_owned()), "{path}");
            assert_eq!(fs::read_to_string(on(&lamp.0, path)).unwrap(), text);
            assert_eq!(mode(path), 0o644);
        }
        assert!(wrote.contains(&SETUP_MARKER.to_owned()));
        assert_eq!(fs::read(on(&lamp.0, SETUP_MARKER)).unwrap(), b"");
        assert_eq!(mode(SETUP_MARKER), 0o600);
        assert_eq!(mode("/var/lib/lampos-setup"), 0o700);
    }

    /// #2937: a moved lamp put back into setup has setup's plan and no
    /// marker. Prepare leaves it so, and setup opens its hotspot; nor does
    /// it write the plan over a lamp's own.
    #[test]
    fn prepare_never_marks_set_up_a_lamp_reset_into_setup() {
        let lamp = v1_lamp();
        prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        fs::remove_file(on(&lamp.0, SETUP_MARKER)).unwrap();
        lamp.file(SETUP_RESET_PLAN, "{\"mine\":true}\n");
        let again = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert!(absent(&on(&lamp.0, SETUP_MARKER)));
        assert_eq!(
            fs::read_to_string(on(&lamp.0, SETUP_RESET_PLAN)).unwrap(),
            "{\"mine\":true}\n"
        );
    }

    /// #2975 (the Reviewer-lead's P3): a prepare cut short between setup's
    /// marker and its plan never leaves a plan without a marker, which would
    /// read as a lamp put back into setup and open its hotspot by itself. Cut
    /// here by a folder where the plan's temporary goes: the marker is there,
    /// the plan not, and the next prepare completes it, the lamp set up.
    #[test]
    fn a_prepare_cut_between_the_marker_and_the_plan_leaves_the_lamp_set_up() {
        let lamp = v1_lamp();
        let blocker = on(&lamp.0, SETUP_RESET_PLAN).with_extension("keeper-new");
        fs::create_dir_all(&blocker).unwrap();
        assert!(prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).is_err());
        assert!(!absent(&on(&lamp.0, SETUP_MARKER)), "the marker first");
        assert!(absent(&on(&lamp.0, SETUP_RESET_PLAN)));
        fs::remove_dir(&blocker).unwrap();
        let again = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        assert!(again.contains(&SETUP_RESET_PLAN.to_owned()), "{again:?}");
        assert!(!again.contains(&SETUP_MARKER.to_owned()), "{again:?}");
        assert!(!absent(&on(&lamp.0, SETUP_MARKER)));
        // And a whole prepare writes the marker before the plan.
        let lamp = v1_lamp();
        let wrote = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        let at = |path: &str| wrote.iter().position(|written| written == path).unwrap();
        assert!(at(SETUP_MARKER) < at(SETUP_RESET_PLAN), "{wrote:?}");
    }

    /// El on #2975: a moved lamp's reset plan that is byte for byte one an
    /// earlier build shipped is written over with this build's, so a store
    /// added since would be wiped too; one changed by hand is
    /// kept; and one already this build's is left.
    #[test]
    fn a_shipped_reset_plan_is_written_over_and_a_changed_one_kept() {
        let old = "{\"marker\":\"/var/lib/lampos-setup/done\",\"wipe\":[]}\n";
        let sum: String = sha2::Sha256::digest(old.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let shipped = [sum.as_str()];
        let lamp = v1_lamp();
        prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        lamp.file(SETUP_RESET_PLAN, old);
        let mut said = Vec::new();
        let wrote = prepare_with(
            &lamp.0,
            &lamp.0.join("release"),
            &shipped,
            &mut yes,
            &mut |line| said.push(line.to_owned()),
        )
        .unwrap();
        assert_eq!(wrote, [SETUP_RESET_PLAN], "{said:?}");
        let (_, plan, _) = SETUP_SETTINGS[1];
        assert_eq!(
            fs::read_to_string(on(&lamp.0, SETUP_RESET_PLAN)).unwrap(),
            plan
        );
        // This build's already: nothing; and the marker is untouched.
        let again = prepare_with(
            &lamp.0,
            &lamp.0.join("release"),
            &shipped,
            &mut yes,
            &mut |_| {},
        )
        .unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert!(!absent(&on(&lamp.0, SETUP_MARKER)));
        // Changed by hand: kept.
        let mine = old.replace("[]", "[\"/var/lib/mine\"]");
        lamp.file(SETUP_RESET_PLAN, &mine);
        let kept = prepare_with(
            &lamp.0,
            &lamp.0.join("release"),
            &shipped,
            &mut yes,
            &mut |_| {},
        )
        .unwrap();
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(
            fs::read_to_string(on(&lamp.0, SETUP_RESET_PLAN)).unwrap(),
            mine
        );
    }

    #[test]
    fn prepare_keeps_what_a_lamp_already_has() {
        let lamp = v1_lamp();
        lamp.file(PROFILE, "{\"unit_id\":\"lamp-a63e\"}\n");
        lamp.file("/etc/lampOS/voice/provider.json", "mine\n");
        let wrote = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        assert!(!wrote.contains(&PROFILE.to_owned()));
        assert!(!wrote.contains(&"/etc/lampOS/voice/provider.json".to_owned()));
        assert_eq!(
            fs::read_to_string(on(&lamp.0, PROFILE)).unwrap(),
            "{\"unit_id\":\"lamp-a63e\"}\n"
        );
        assert_eq!(
            fs::read_to_string(on(&lamp.0, "/etc/lampOS/voice/provider.json")).unwrap(),
            "mine\n"
        );
        let again = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        assert!(
            again.is_empty(),
            "a second prepare writes nothing: {again:?}"
        );
    }

    #[test]
    fn a_prepared_lamp_is_not_refused_for_its_account() {
        let lamp = v1_lamp();
        prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        lamp.file(
            "/etc/passwd",
            "root:x:0:0::/root:/bin/bash\nlampos:x:990:990::/:/bin/false\n",
        );
        let again = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap();
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn a_file_that_appears_is_not_replaced() {
        let lamp = v1_lamp();
        let path = on(&lamp.0, "/etc/lampOS/voice/provider.json");
        lamp.file("/etc/lampOS/voice/provider.json", "theirs\n");
        assert!(write_new(&path, "ours\n", 0o600).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
        assert!(absent(&path.with_extension("keeper-new")));
    }

    /// #2163: a V1 lamp with no poses is prepared, and its poses are left
    /// to lamp-body-measure.service after cutover.
    #[test]
    fn a_lamp_without_poses_is_prepared_and_left_to_the_seated_measure() {
        let lamp = v1_lamp();
        fs::remove_file(on(&lamp.0, LAMP_ENV)).unwrap();
        let mut said = Vec::new();
        let wrote = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |line| {
            said.push(line.to_owned())
        })
        .unwrap();
        assert_eq!(wrote[0], PROFILE);
        assert!(
            said.iter()
                .any(|line| line.contains("lamp-body-measure.service"))
        );
        let profile: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(on(&lamp.0, PROFILE)).unwrap()).unwrap();
        assert!(profile["substitutions"].get("BODY_POSES").is_none());
        assert_eq!(profile["gaze"], "not measured");
        assert!(!absent(&on(&lamp.0, "/var/lib/lampos-locks/motor")));
    }

    #[test]
    fn a_refusal_writes_nothing() {
        let lamp = v1_lamp();
        lamp.file(LAMP_ENV, "BODY_POSES=\"--folded-arm 1,2\"\n");
        let why = prepare(&lamp.0, &lamp.0.join("release"), &mut yes, &mut |_| {}).unwrap_err();
        assert!(why.contains("--home"), "{why}");
        assert!(absent(&on(&lamp.0, PROFILE)));
        assert!(absent(&on(&lamp.0, "/etc/lampOS/voice/provider.json")));
        assert!(absent(&on(&lamp.0, "/var/lib/lampos-locks")));

        let lamp = v1_lamp();
        let why = prepare(
            &lamp.0,
            &lamp.0.join("release"),
            &mut |_, _| false,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(why.contains("Speaker"), "{why}");
        assert!(absent(&on(&lamp.0, PROFILE)));
    }
}
