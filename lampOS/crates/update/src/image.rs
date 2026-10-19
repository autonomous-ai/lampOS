//! The image's own files a build carries (#1369, #1680 step 6): the units,
//! udev rules, ALSA configuration and drop-ins `robots/lamp/rootfs/` (and a
//! variant's `robots/lamp/overrides/<variant>/rootfs/`) lays down, which
//! `install` never placed. Off unless `LAMP_UPDATE_IMAGE=on` is in
//! lamp-update.service's environment.
//!
//! They are written by `lamp-update-apply` with the settings
//! (crate::apply), by the same three-way rule, except that the build's
//! before is also every SHA-256 the image has shipped at that path for the
//! lamp's variant (`deploy/image/shipped.json`): a lamp's file that is none
//! of them, and not what an update wrote, is the lamp's own and kept. A
//! drop-in a lamp or a person made (lamp-4ace's `30-local-gain.conf`) is
//! never one of [`manifest::IMAGE_FILES`], so it is never touched.
//!
//! After a write: `systemctl daemon-reload` for a unit or a unit's drop-in,
//! `udevadm control --reload` for a rule (no trigger: a rule is used at the
//! next plug or boot), and the speaker's level unit started once. journald
//! is never restarted nor signalled, since that ends every part's log
//! stream (the first builder on #1680, 15 Oct): its drop-in is read at the
//! next boot. `/etc/asound.conf` restarts the voice, which a round does
//! only at a quiet moment (crate::quiet), as every switch of the voice.
use crate::manifest::{self, ImageFile, Setting};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// The switch, in lamp-update.service's environment: `on`, or off.
pub const SWITCH: &str = "LAMP_UPDATE_IMAGE";
/// Where the image says which variant it is: none, empty or `standard` is
/// the standard lamp (scripts/imager, `apply-overrides.py`).
pub const PROFILE: &str = "/etc/autonomous/hardware-profile";
pub const STANDARD: &str = "standard";
/// The speaker's level unit: started once when written.
pub const SPEAKER_UNIT: &str = "lamp-cmedia-speaker.service";
/// ALSA's configuration: read by the voice when it opens its devices.
pub const ASOUND: &str = "/etc/asound.conf";
/// journald's drop-in: read at the next boot, never by a restart.
pub const JOURNALD_DROP_IN: &str = "/etc/systemd/journald.conf.d/zz-lamp-journal.conf";
const UNITS: &str = "/etc/systemd/system/";
const RULES: &str = "/etc/udev/rules.d/";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The lamp's variant, from [`PROFILE`] under `root`. None: a name that is
/// not one, and nothing of the image is carried.
pub fn variant(root: &Path) -> io::Result<Option<String>> {
    let at = root.join(PROFILE.trim_start_matches('/'));
    let name = match fs::read_to_string(at) {
        Ok(text) => text.trim().to_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if name.is_empty() {
        return Ok(Some(STANDARD.to_owned()));
    }
    Ok(manifest::is_variant(&name).then_some(name))
}

/// The build's files for this variant, as settings, and what the image
/// shipped at each path.
pub fn for_variant(
    image: &[ImageFile],
    variant: &str,
) -> (Vec<Setting>, BTreeMap<String, Vec<String>>) {
    let ours: Vec<&ImageFile> = image
        .iter()
        .filter(|file| file.variant == variant)
        .collect();
    (
        ours.iter().map(|file| file.setting()).collect(),
        ours.iter()
            .map(|file| (file.path.clone(), file.shipped.clone()))
            .collect(),
    )
}

/// The part a written file restarts, if any.
pub fn restarts(path: &str) -> Option<&'static str> {
    (path == ASOUND).then_some("lamp-voice.service")
}

/// What is run once these of the image's files are written or put back, in
/// order. The second is whether a failure is only said: the speaker's level
/// unit fails on a lamp without its card.
pub fn after(paths: &[String]) -> Vec<(Vec<String>, bool)> {
    let mut commands = Vec::new();
    let has = |test: &dyn Fn(&str) -> bool| paths.iter().any(|path| test(path));
    if has(&|path| path.starts_with(UNITS)) {
        commands.push((vec!["systemctl".into(), "daemon-reload".into()], false));
    }
    if has(&|path| path.starts_with(RULES)) {
        commands.push((
            vec!["udevadm".into(), "control".into(), "--reload".into()],
            false,
        ));
    }
    if has(&|path| path == format!("{UNITS}{SPEAKER_UNIT}")) {
        commands.push((
            vec!["systemctl".into(), "start".into(), SPEAKER_UNIT.into()],
            true,
        ));
    }
    commands
}

/// What is said of a written file nothing reloads now.
pub fn later(path: &str) -> Option<String> {
    (path == JOURNALD_DROP_IN).then(|| {
        format!("{path} is read by journald at the next boot; journald is never restarted")
    })
}

/// Every command the apply path runs passes here first: none may name
/// journald, whose restart or signal ends every part's log stream.
pub fn guard(command: &[&str]) -> Result<(), String> {
    if command.iter().any(|part| part.contains("journald")) {
        return Err(format!(
            "{:?} is refused: an update never restarts or signals journald",
            command.join(" ")
        ));
    }
    Ok(())
}

/// `deploy/image/shipped.json` as this program was built with it.
pub const SHIPPED: &str = include_str!("../../../deploy/image/shipped.json");

/// `deploy/image/shipped.json`: for each variant, each path's SHA-256s the
/// image has shipped.
pub type Shipped = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// The image's files of `robots/lamp` (`lamp`), each variant merged as
/// `apply-overrides.py` merges it: the variant's own file where it has one,
/// else the standard one. Each file's SHA-256 must be among those
/// `shipped` lists for it, so the next build knows it as the image's.
pub fn from_tree(lamp: &Path, shipped: &Shipped) -> io::Result<Vec<ImageFile>> {
    let invalid = |why: String| io::Error::new(io::ErrorKind::InvalidData, why);
    let mut variants = vec![(STANDARD.to_owned(), None::<PathBuf>)];
    let overrides = lamp.join("overrides");
    if overrides.is_dir() {
        let mut names = Vec::new();
        for entry in fs::read_dir(&overrides)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().join("rootfs").is_dir() && manifest::is_variant(&name) {
                names.push(name);
            }
        }
        names.sort();
        variants.extend(
            names
                .into_iter()
                .map(|name| (name.clone(), Some(overrides.join(name).join("rootfs")))),
        );
    }
    let base = lamp.join("rootfs");
    let mut files = Vec::new();
    for (variant, own) in &variants {
        for path in manifest::IMAGE_FILES {
            let relative = path.trim_start_matches('/');
            let at = own
                .as_ref()
                .map(|own| own.join(relative))
                .filter(|at| at.is_file())
                .unwrap_or_else(|| base.join(relative));
            let bytes = match fs::read(&at) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let text = String::from_utf8(bytes)
                .map_err(|_| invalid(format!("{} is not text", at.display())))?;
            let sha256 = manifest::sha256_hex(text.as_bytes());
            let listed = shipped
                .get(variant)
                .and_then(|paths| paths.get(*path))
                .cloned()
                .unwrap_or_default();
            if !listed.contains(&sha256) {
                return Err(invalid(format!(
                    "{path} of {variant} is {sha256}, which deploy/image/shipped.json does not list"
                )));
            }
            files.push(ImageFile {
                variant: variant.clone(),
                path: (*path).to_owned(),
                text,
                sha256,
                shipped: listed,
            });
        }
    }
    manifest::check_image(&files).map_err(|why| invalid(why.0))?;
    Ok(files)
}

/// [`Shipped`] from its text.
pub fn shipped_from(text: &str) -> io::Result<Shipped> {
    #[derive(Deserialize)]
    struct File {
        variants: Shipped,
    }
    serde_json::from_str::<File>(text)
        .map(|file| file.variants)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../robots/lamp")
    }

    fn shipped() -> Shipped {
        shipped_from(SHIPPED).unwrap()
    }

    /// Each file the image lays down today is listed as shipped: change one
    /// in `robots/lamp`, and its new SHA-256 is added beside the old ones.
    #[test]
    fn every_image_file_of_every_variant_is_listed_as_shipped() {
        let files = from_tree(&tree(), &shipped()).unwrap();
        let of = |variant: &str, path: &str| {
            files
                .iter()
                .find(|file| file.variant == variant && file.path == path)
                .unwrap_or_else(|| panic!("{path} of {variant}"))
        };
        let standard = of(STANDARD, "/etc/systemd/system/lamp-cmedia-speaker.service");
        let pro = of("pro", "/etc/systemd/system/lamp-cmedia-speaker.service");
        assert!(
            standard.text.contains("sset Speaker 9"),
            "{}",
            standard.text
        );
        assert!(pro.text.contains("sset Speaker 6"), "{}", pro.text);
        // A variant without its own file has the standard one.
        assert_eq!(of("pro", ASOUND).text, of(STANDARD, ASOUND).text);
        assert_ne!(of("pro-xvf3800", ASOUND).text, of(STANDARD, ASOUND).text);
        // Only the respeaker image has its own rule and unit.
        assert!(
            files
                .iter()
                .filter(|file| file.path == "/etc/udev/rules.d/90-respeaker-lite.rules")
                .all(|file| file.variant == "pro-respeaker-lite")
        );
        for file in &files {
            assert!(file.shipped.contains(&file.sha256), "{}", file.path);
        }
        // Never V1's HAL.
        assert!(!files.iter().any(|file| file.path.starts_with("/opt/hal")));
    }

    #[test]
    fn a_file_not_listed_as_shipped_is_refused() {
        let mut shipped = shipped();
        shipped
            .get_mut(STANDARD)
            .unwrap()
            .get_mut(ASOUND)
            .unwrap()
            .clear();
        let error = from_tree(&tree(), &shipped).unwrap_err();
        assert!(error.to_string().contains("does not list"), "{error}");
    }

    #[test]
    fn the_variant_is_the_images_and_none_is_standard() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(variant(folder.path()).unwrap().as_deref(), Some(STANDARD));
        let at = folder.path().join(PROFILE.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(&at, "pro\n").unwrap();
        assert_eq!(variant(folder.path()).unwrap().as_deref(), Some("pro"));
        fs::write(&at, "\n").unwrap();
        assert_eq!(variant(folder.path()).unwrap().as_deref(), Some(STANDARD));
        fs::write(&at, "../pro").unwrap();
        assert_eq!(variant(folder.path()).unwrap(), None);
    }

    /// The first builder on #1680, 19:48: an update never restarts or
    /// signals journald.
    #[test]
    fn journald_is_never_restarted_nor_signalled() {
        for command in [
            &["systemctl", "restart", "systemd-journald.service"][..],
            &["systemctl", "restart", "systemd-journald"],
            &["systemctl", "kill", "systemd-journald"],
            &["systemctl", "kill", "-s", "HUP", "systemd-journald.service"],
            &["systemctl", "try-restart", "systemd-journald.socket"],
            &["pkill", "-HUP", "systemd-journald"],
        ] {
            assert!(guard(command).is_err(), "{command:?}");
        }
        // What every carried file asks, all at once: nothing of journald.
        let all: Vec<String> = manifest::IMAGE_FILES.iter().map(|p| (*p).into()).collect();
        for (command, _) in after(&all) {
            let parts: Vec<&str> = command.iter().map(String::as_str).collect();
            guard(&parts).unwrap();
        }
        assert!(after(&[JOURNALD_DROP_IN.into()]).is_empty());
        assert!(later(JOURNALD_DROP_IN).unwrap().contains("next boot"));
        for path in manifest::IMAGE_FILES {
            assert!(!restarts(path).is_some_and(|unit| unit.contains("journald")));
        }
        guard(&["systemctl", "daemon-reload"]).unwrap();
    }

    #[test]
    fn a_unit_reloads_systemd_a_rule_reloads_udev_and_asound_restarts_the_voice() {
        let commands = |paths: &[&str]| {
            after(&paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>())
                .into_iter()
                .map(|(command, only_said)| (command.join(" "), only_said))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            commands(&[
                "/etc/udev/rules.d/99-lamp-device.rules",
                "/etc/systemd/system/lamp-cmedia-speaker.service",
            ]),
            [
                ("systemctl daemon-reload".to_owned(), false),
                ("udevadm control --reload".to_owned(), false),
                (
                    "systemctl start lamp-cmedia-speaker.service".to_owned(),
                    true
                ),
            ]
        );
        assert_eq!(
            commands(&["/etc/systemd/system/logrotate.service.d/zz-lamp-ramlog.conf"]),
            [("systemctl daemon-reload".to_owned(), false)]
        );
        assert!(commands(&[ASOUND]).is_empty());
        assert_eq!(restarts(ASOUND), Some("lamp-voice.service"));
        assert_eq!(restarts("/etc/udev/rules.d/99-lamp-device.rules"), None);
    }
}
