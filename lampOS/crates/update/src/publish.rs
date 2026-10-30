//! The manifest of a build, made where it is built: every lampOS binary
//! in a folder (CI's `arm64-out`, with its `SHA256SUMS`), every unit of
//! every lamp's own folder (`deploy/<lamp>/`, a folder with a `LAMP.md`),
//! ordinary binary-backed services in `deploy/product/`, and the product's
//! skills (`skills/`, each of
//! [`PRODUCT_SKILLS`](crate::install::PRODUCT_SKILLS)) as one file,
//! [`product_skills::BUNDLE`]. The units are copied beside the binaries as
//! `<lamp>.<unit>`, and the skills written there, so that the folder is
//! what is published.
use crate::{
    companion, install,
    manifest::{self, File, ImageFile, Manifest, Model, SCHEMA, Setting, Unit},
    product_skills, unit,
};
use std::{collections::BTreeMap, fs, io, path::Path};

fn invalid(why: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}

/// The manifest, written as `manifest.json` into `binaries`, with the units
/// copied there and the product skills of `skills` written there.
///
/// `min_updater`, when given, is the oldest updater that can apply it. It
/// must not be above the updater the build carries, or a lamp could never
/// climb to it: so the build must carry `lamp-update`, and the level is
/// this program's own ([`manifest::UPDATER`]), which is the carried one's
/// when the manifest is made by the build's own `lamp-update`, as it is.
///
/// `models`, when there are any, are carried in the manifest (#1680), and
/// `min_updater` is then at least [`manifest::MODELS_UPDATER`], so that an
/// older updater climbs before it reads them.
#[allow(clippy::too_many_arguments)]
pub fn make(
    binaries: &Path,
    deploy: &Path,
    skills: &Path,
    channel: &str,
    sequence: u64,
    commit: &str,
    files_url: &str,
    min_updater: Option<u64>,
    models: Vec<Model>,
) -> io::Result<Manifest> {
    make_with(
        binaries,
        deploy,
        skills,
        channel,
        sequence,
        commit,
        files_url,
        min_updater,
        models,
        Vec::new(),
        Vec::new(),
        None,
    )
}

/// [`make`], with the product's `settings` carried too (#1680):
/// `min_updater` is then at least [`manifest::SETTINGS_UPDATER`]; the
/// image's own files (#1369, crate::image::from_tree), with it then at
/// least [`manifest::IMAGE_UPDATER`]; and setup's reset plan (#806), with
/// it then at least [`manifest::RESET_PLAN_UPDATER`].
#[allow(clippy::too_many_arguments)]
pub fn make_with(
    binaries: &Path,
    deploy: &Path,
    skills: &Path,
    channel: &str,
    sequence: u64,
    commit: &str,
    files_url: &str,
    min_updater: Option<u64>,
    models: Vec<Model>,
    settings: Vec<Setting>,
    image: Vec<ImageFile>,
    reset_plan: Option<Setting>,
) -> io::Result<Manifest> {
    let min_updater = if models.is_empty() {
        min_updater
    } else {
        Some(min_updater.unwrap_or(1).max(manifest::MODELS_UPDATER))
    };
    let min_updater = if settings.is_empty() {
        min_updater
    } else {
        Some(min_updater.unwrap_or(1).max(manifest::SETTINGS_UPDATER))
    };
    let min_updater = if image.is_empty() {
        min_updater
    } else {
        Some(min_updater.unwrap_or(1).max(manifest::IMAGE_UPDATER))
    };
    let mut min_updater = if reset_plan.is_none() {
        min_updater
    } else {
        Some(min_updater.unwrap_or(1).max(manifest::RESET_PLAN_UPDATER))
    };
    let mut files = Vec::new();
    let sums = fs::read_to_string(binaries.join("SHA256SUMS")).ok();
    for entry in fs::read_dir(binaries)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // The skills' file is made below, never taken as a binary (one
        // left by an earlier run is written again).
        if !manifest::is_binary_name(&name)
            || name == product_skills::BUNDLE
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        let sha256 = manifest::sha256_hex(&bytes);
        // CI's own list, where there is one, must agree.
        if let Some(sums) = &sums
            && !sums
                .lines()
                .any(|line| line.split_whitespace().eq([sha256.as_str(), name.as_str()]))
        {
            return Err(invalid(format!("{name} is not in SHA256SUMS as {sha256}")));
        }
        files.push(File {
            name,
            sha256,
            size: bytes.len() as u64,
        });
    }
    let bundle = product_skills::make(skills, install::PRODUCT_SKILLS)?;
    let bytes = product_skills::bytes(&bundle)?;
    fs::write(binaries.join(product_skills::BUNDLE), &bytes)?;
    files.push(File {
        name: product_skills::BUNDLE.to_owned(),
        sha256: manifest::sha256_hex(&bytes),
        size: bytes.len() as u64,
    });
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let mut units = Vec::new();
    for lamp in fs::read_dir(deploy)? {
        let lamp = lamp?;
        let name = lamp.file_name().to_string_lossy().into_owned();
        let product = name == manifest::PRODUCT_LAMP;
        if !manifest::is_token(&name) || (!product && !lamp.path().join("LAMP.md").is_file()) {
            continue;
        }
        // A part's templates, sockets and timers, the product's only, are
        // read with the units they start (crate::companion).
        let mut texts = BTreeMap::new();
        for entry in fs::read_dir(lamp.path())? {
            let entry = entry?;
            let unit_name = entry.file_name().to_string_lossy().into_owned();
            let companion = product && companion::kind(&unit_name).is_some();
            if !(manifest::is_unit_name(&unit_name) || companion) || !entry.file_type()?.is_file() {
                continue;
            }
            // The updater's own service and fixed recovery helpers keep their
            // existing handover boundary; this carrier is for ordinary parts.
            if product && unit_name == crate::round::OWN_UNIT {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            // Only a part installed by build: the others are not updated.
            if let Ok(text) = String::from_utf8(bytes) {
                texts.insert(unit_name, text);
            }
        }
        for (unit_name, text) in &texts {
            let carried = match companion::kind(unit_name) {
                Some(_) => {
                    companion::part(unit_name, text, |name| texts.get(name).map(String::as_str))
                        .is_some()
                }
                None => unit::program(text).is_some() || unit::linked(text).is_some(),
            };
            if !carried {
                continue;
            }
            let (unit_name, bytes) = (unit_name.clone(), text.as_bytes());
            let unit = Unit {
                lamp: name.clone(),
                unit: manifest::manifest_name(&unit_name),
                sha256: manifest::sha256_hex(bytes),
                size: bytes.len() as u64,
            };
            fs::write(binaries.join(unit.file_name()), bytes)?;
            units.push(unit);
        }
    }
    units.sort_by(|a, b| (&a.lamp, &a.unit).cmp(&(&b.lamp, &b.unit)));
    if units.iter().any(|unit| unit.lamp == manifest::PRODUCT_LAMP) {
        min_updater = Some(
            min_updater
                .unwrap_or(1)
                .max(manifest::PRODUCT_UNITS_UPDATER),
        );
    }
    // Every build with the product's units may add one a lamp lacks, and
    // is switched in this updater's order: an older updater climbs first.
    if units.iter().any(|unit| unit.lamp == manifest::PRODUCT_LAMP) {
        min_updater = Some(
            min_updater
                .unwrap_or(1)
                .max(manifest::ADDS_UPDATER)
                .max(manifest::ORDER_UPDATER),
        );
    }
    // A template by its name on the lamp (`manifest::lamp_name`).
    if units
        .iter()
        .any(|unit| companion::kind(&unit.on_lamp()).is_some())
    {
        min_updater = Some(min_updater.unwrap_or(1).max(manifest::COMPANIONS_UPDATER));
    }
    if let Some(needed) = min_updater {
        if !files.iter().any(|file| file.name == "lamp-update") {
            return Err(invalid(format!(
                "min_updater {needed}: the build carries no lamp-update, so no lamp could climb to it"
            )));
        }
        if needed > manifest::UPDATER {
            return Err(invalid(format!(
                "min_updater {needed} is above the updater the build carries, updater {}",
                manifest::UPDATER
            )));
        }
    }
    let made = Manifest {
        schema: SCHEMA,
        channel: channel.to_owned(),
        sequence,
        commit: commit.to_owned(),
        files_url: files_url.to_owned(),
        files,
        units,
        min_updater,
        models,
        settings,
        image,
        reset_plan: reset_plan.map(Box::new),
    };
    let mut bytes = serde_json::to_vec_pretty(&made).map_err(io::Error::other)?;
    bytes.push(b'\n');
    manifest::parse(&bytes).map_err(|why| invalid(why.0))?;
    fs::write(binaries.join("manifest.json"), bytes)?;
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_services_and_linked_lamp_overrides_are_carried_with_the_level_that_adds() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(deploy.join(manifest::PRODUCT_LAMP)).unwrap();
        fs::create_dir_all(deploy.join("lamp-owned")).unwrap();
        fs::write(deploy.join("lamp-owned/LAMP.md"), "owned\n").unwrap();
        fs::write(out.join("lamp-update"), "update").unwrap();
        fs::write(out.join("lamp-voice"), "voice").unwrap();
        fs::write(out.join("lamp-body"), "body").unwrap();
        let product = "[Service]\nExecStart=/opt/lampOS/releases/current/lamp-voice serve\n";
        let owned = format!("{product}Environment=LAMP_BUS_ROOT=/run/owned\n");
        for (name, text) in [
            ("lamp-voice.service", product.to_owned()),
            (
                "lamp-body.service",
                format!(
                    "[Service]\nExecStart=/opt/lampOS/body/{}/lamp-body serve\n",
                    "a".repeat(64)
                ),
            ),
            (
                "lamp-update.service",
                "[Service]\nExecStart=/opt/lampOS/update/current/lamp-update serve\n".into(),
            ),
            (
                "lamp-external.service",
                "[Service]\nExecStart=/usr/bin/true\n".into(),
            ),
            ("lamp-voice.socket", "[Socket]\nListenStream=1\n".into()),
            ("lampos.target", "[Unit]\n".into()),
        ] {
            fs::write(deploy.join(manifest::PRODUCT_LAMP).join(name), text).unwrap();
        }
        fs::write(deploy.join("lamp-owned/lamp-voice.service"), &owned).unwrap();
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &"c".repeat(40),
            "https://channel.test/build-7/",
            None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(made.min_updater, Some(manifest::ORDER_UPDATER));
        // An updater of level 8, which restarts lamp-controls before the
        // voice, reads the build as too new for it and takes only the
        // lamp-update it carries (second-P1); this updater reads it whole.
        let bytes = serde_json::to_vec(&made).unwrap();
        let too_old = manifest::too_old(&bytes, manifest::ADDS_UPDATER).unwrap();
        assert_eq!(too_old.needed, manifest::ORDER_UPDATER);
        assert_eq!(too_old.carried.unwrap().name, "lamp-update");
        assert!(manifest::too_old(&bytes, manifest::UPDATER).is_none());
        assert_eq!(made.units.len(), 3);
        let product_voice = made
            .units
            .iter()
            .find(|unit| unit.lamp == manifest::PRODUCT_LAMP && unit.unit == "lamp-voice.service")
            .unwrap();
        assert_eq!(
            fs::read(out.join(product_voice.file_name())).unwrap(),
            product.as_bytes()
        );
        assert_eq!(
            product_voice.sha256,
            manifest::sha256_hex(product.as_bytes())
        );
        let lamp_voice = made
            .units
            .iter()
            .find(|unit| unit.lamp == "lamp-owned")
            .unwrap();
        assert_eq!(
            fs::read(out.join(lamp_voice.file_name())).unwrap(),
            owned.as_bytes()
        );
        assert_eq!(
            manifest::parse(&fs::read(out.join("manifest.json")).unwrap()).unwrap(),
            made
        );
        fs::remove_file(out.join("lamp-update")).unwrap();
        let error = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            8,
            &"c".repeat(40),
            "https://channel.test/build-8/",
            None,
            Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("carries no lamp-update"));
    }

    #[test]
    fn a_parts_templates_sockets_and_timers_are_carried_with_the_level_that_adds() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(deploy.join(manifest::PRODUCT_LAMP)).unwrap();
        fs::create_dir_all(deploy.join("lamp-owned")).unwrap();
        fs::write(deploy.join("lamp-owned/LAMP.md"), "owned\n").unwrap();
        for binary in ["lamp-update", "lamp-setup", "lamp-records"] {
            fs::write(out.join(binary), binary).unwrap();
        }
        let setup = "[Service]\nExecStart=/opt/lampOS/setup/current/lamp-setup serve\n";
        let records = "[Service]\nExecStart=/opt/lampOS/records/current/lamp-records check\n";
        for (name, text) in [
            ("lamp-setup.service", setup),
            (
                "lamp-setup-wifi@.service",
                "[Service]\nExecStart=/opt/lampOS/setup/current/lamp-setup wifi\n",
            ),
            ("lamp-setup-wifi.socket", "[Socket]\nAccept=yes\n"),
            ("lamp-records.service", records),
            ("lamp-records.timer", "[Timer]\nOnCalendar=hourly\n"),
            // Fixed programs that no round switches.
            (
                "lamp-keeper-restart@.service",
                "[Service]\nExecStart=/usr/local/bin/lamp-keeper restart-asked\n",
            ),
            ("lamp-keeper-restart.socket", "[Socket]\nAccept=yes\n"),
            (
                "lamp-update-back.service",
                "[Service]\nExecStart=/usr/local/lib/lampos/lamp-update-back back\n",
            ),
            ("lamp-update-back.timer", "[Timer]\nOnBootSec=2min\n"),
            ("lampos.target", "[Unit]\n"),
        ] {
            fs::write(deploy.join(manifest::PRODUCT_LAMP).join(name), text).unwrap();
        }
        // A lamp's own folder carries services only.
        fs::write(deploy.join("lamp-owned/lamp-records.timer"), "[Timer]\n").unwrap();
        fs::write(deploy.join("lamp-owned/lamp-records.service"), records).unwrap();
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &"c".repeat(40),
            "https://channel.test/build-7/",
            None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(made.min_updater, Some(manifest::ORDER_UPDATER));
        let units: Vec<String> = made.units.iter().map(Unit::file_name).collect();
        assert_eq!(
            units,
            [
                "lamp-owned.lamp-records.service",
                "product.lamp-records.service",
                "product.lamp-records.timer",
                "product.lamp-setup-wifi.socket",
                "product.lamp-setup-wifi.template.service",
                "product.lamp-setup.service",
            ]
        );
        for unit in &made.units {
            assert!(out.join(unit.file_name()).is_file(), "{}", unit.file_name());
        }
    }

    #[test]
    fn binaries_and_each_lamps_units_but_nothing_else() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("lamp-voice"), b"voice").unwrap();
        fs::write(out.join("lamp-body"), b"body").unwrap();
        fs::write(out.join("schedule_check"), b"example").unwrap();
        fs::write(out.join("lamp-voice.d"), b"dependencies").unwrap();
        let sums = format!(
            "{}  lamp-body\n{}  lamp-voice\n",
            manifest::sha256_hex(b"body"),
            manifest::sha256_hex(b"voice")
        );
        fs::write(out.join("SHA256SUMS"), sums).unwrap();
        let sha = "a".repeat(64);
        for (lamp, owned) in [("lamp-4ace", true), ("units", false)] {
            fs::create_dir_all(deploy.join(lamp)).unwrap();
            if owned {
                fs::write(deploy.join(lamp).join("LAMP.md"), b"owner: first\n").unwrap();
            }
            fs::write(
                deploy.join(lamp).join("lamp-voice.service"),
                format!("[Service]\nExecStart=/opt/lampOS/releases/{sha}/lamp-voice serve\n"),
            )
            .unwrap();
            fs::write(
                deploy.join(lamp).join("lamp-air.service"),
                "[Service]\nExecStart=/usr/bin/true\n",
            )
            .unwrap();
        }
        let url = "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-7/";
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &"c".repeat(40),
            url,
            None,
            Vec::new(),
        )
        .unwrap();
        let names: Vec<&str> = made.files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, ["lamp-body", "lamp-product-skills", "lamp-voice"]);
        // The skills' file is signed with the manifest, as a binary is, and
        // holds each product skill.
        let bundle = fs::read(out.join(product_skills::BUNDLE)).unwrap();
        let listed = &made.files[1];
        assert_eq!(listed.sha256, manifest::sha256_hex(&bundle));
        assert_eq!(listed.size, bundle.len() as u64);
        let bundle = product_skills::parse(&bundle).unwrap();
        let mut product: Vec<&str> = install::PRODUCT_SKILLS.to_vec();
        product.sort_unstable();
        assert_eq!(bundle.skills, product);
        // Made again, the file left by the first run is not taken for a
        // binary.
        let again = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &"c".repeat(40),
            url,
            None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(again, made);
        assert_eq!(made.units.len(), 1);
        assert_eq!(made.units[0].file_name(), "lamp-4ace.lamp-voice.service");
        assert!(out.join("lamp-4ace.lamp-voice.service").is_file());
        let written = fs::read(out.join("manifest.json")).unwrap();
        assert_eq!(manifest::parse(&written).unwrap(), made);
        assert!(!String::from_utf8_lossy(&written).contains("min_updater"));

        // A binary that SHA256SUMS does not know as it is: refused.
        fs::write(out.join("lamp-body"), b"changed").unwrap();
        assert!(
            make(
                &out,
                &deploy,
                &skills,
                "lamp-voice",
                7,
                &"c".repeat(40),
                url,
                None,
                Vec::new()
            )
            .is_err()
        );

        // Asked for, without a lamp-update in the build: refused, since no
        // lamp could climb to it.
        fs::write(out.join("lamp-body"), b"body").unwrap();
        let level = manifest::UPDATER;
        let commit = "c".repeat(40);
        let why = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &commit,
            url,
            Some(level),
            Vec::new(),
        )
        .unwrap_err();
        assert!(why.to_string().contains("carries no lamp-update"), "{why}");
        // With one: the oldest updater that can apply it is written in.
        fs::write(out.join("lamp-update"), b"update").unwrap();
        fs::remove_file(out.join("SHA256SUMS")).unwrap();
        let made = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &commit,
            url,
            Some(level),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(made.min_updater, Some(level));
        let written = fs::read(out.join("manifest.json")).unwrap();
        assert_eq!(manifest::parse(&written).unwrap().min_updater, Some(level));
        // Above the updater it carries: refused, nothing written over.
        let why = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &commit,
            url,
            Some(level + 1),
            Vec::new(),
        )
        .unwrap_err();
        assert!(why.to_string().contains("above the updater"), "{why}");
        assert_eq!(fs::read(out.join("manifest.json")).unwrap(), written);
        // Never 0: left out instead.
        assert!(
            make(
                &out,
                &deploy,
                &skills,
                "lamp-voice",
                7,
                &commit,
                url,
                Some(0),
                Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn models_are_carried_with_the_updater_they_need() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&deploy).unwrap();
        fs::write(out.join("lamp-eyes"), b"eyes").unwrap();
        fs::write(out.join("lamp-update"), b"update").unwrap();
        let models = crate::install::models().unwrap();
        let url = "https://channel.test/build-7/";
        let commit = "c".repeat(40);
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &commit,
            url,
            None,
            models.clone(),
        )
        .unwrap();
        assert_eq!(made.models, models);
        assert_eq!(made.min_updater, Some(manifest::MODELS_UPDATER));
        let written = fs::read(out.join("manifest.json")).unwrap();
        assert_eq!(manifest::parse(&written).unwrap(), made);
        // Without lamp-update the build could strand a lamp: refused.
        fs::remove_file(out.join("lamp-update")).unwrap();
        let why = make(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            7,
            &commit,
            url,
            None,
            models,
        )
        .unwrap_err();
        assert!(why.to_string().contains("carries no lamp-update"), "{why}");
    }

    #[test]
    fn the_products_settings_are_carried_with_the_updater_they_need() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&deploy).unwrap();
        fs::write(out.join("lamp-update"), b"update").unwrap();
        let settings = crate::install::product_settings();
        let commit = "c".repeat(40);
        let url = "https://channel.test/build-8/";
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make_with(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            8,
            &commit,
            url,
            None,
            Vec::new(),
            settings.clone(),
            Vec::new(),
            Some(crate::install::reset_plan_setting()),
        )
        .unwrap();
        assert_eq!(made.settings, settings);
        // With setup's reset plan, in its own field (#806).
        assert_eq!(made.min_updater, Some(manifest::RESET_PLAN_UPDATER));
        assert_eq!(
            made.reset_plan
                .as_ref()
                .and_then(|plan| plan.text.as_deref()),
            Some(crate::install::RESET_PLAN)
        );
        let written = fs::read(out.join("manifest.json")).unwrap();
        assert_eq!(manifest::parse(&written).unwrap(), made);
        // Every product setting and the tmpfiles, and none of the keys.
        let paths: Vec<&str> = made.settings.iter().map(|s| s.path.as_str()).collect();
        assert!(paths.contains(&"/etc/lampOS/expression/expression.json"));
        assert!(paths.contains(&"/etc/tmpfiles.d/lampos.conf"));
        assert!(
            !paths
                .iter()
                .any(|path| path.contains("credential") || path.contains("key"))
        );
        // The product skills are lamp-product-skills', never a setting.
        assert!(
            !paths
                .iter()
                .any(|path| path.starts_with("/var/lib/lampOS/skills/"))
        );
    }
    /// The Skills Store's key (#3692): carried as a setting at
    /// `install::STORE_KEY_SETTING`, 0644, only when the repository has
    /// the file, and only as one or two keys a lamp would take. Its path is
    /// one the settings writer takes, and that writer's places are pinned
    /// here as build 7071's fixed copy has them (its `manifest.rs:79-84`),
    /// so a lamp still on 7071's `lamp-update-back` writes it.
    #[test]
    fn the_stores_key_is_carried_only_when_the_repository_has_it() {
        assert_eq!(
            manifest::SETTING_FILES,
            ["/etc/lampOS/records.json", "/etc/tmpfiles.d/lampos.conf"]
        );
        assert_eq!(
            manifest::SETTING_FOLDERS,
            [
                "/etc/lampOS/expression/",
                "/etc/lampOS/controls/",
                "/var/lib/lampOS/skills/",
            ]
        );
        assert_eq!(manifest::SETTING_MODES, ["0644", "0600"]);
        let folder = tempfile::tempdir().unwrap();
        let deploy = folder.path().join("deploy");
        assert_eq!(crate::install::store_key_setting(&deploy), Ok(None));
        let source = deploy.join(crate::install::STORE_KEY_SOURCE);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        let two = format!("{}=\n{}=\n", "A".repeat(43), "Q".repeat(43));
        fs::write(&source, &two).unwrap();
        let setting = crate::install::store_key_setting(&deploy).unwrap().unwrap();
        assert_eq!(setting.path, crate::install::STORE_KEY_SETTING);
        assert_eq!(setting.text.as_deref(), Some(two.as_str()));
        assert_eq!(setting.mode.as_deref(), Some("0644"));
        let mut settings = crate::install::product_settings();
        settings.push(setting);
        manifest::check_settings(&settings).unwrap();
        // Carried into a manifest whole.
        let out = folder.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("lamp-update"), b"update").unwrap();
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make_with(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            10,
            &"e".repeat(40),
            "https://channel.test/build-10/",
            None,
            Vec::new(),
            settings.clone(),
            Vec::new(),
            None,
        )
        .unwrap();
        assert_eq!(made.settings, settings);
        // A file a lamp would refuse is no build's.
        for wrong in [
            String::new(),
            "not a key\n".to_owned(),
            format!("{two}{}=\n", "g".repeat(43)),
        ] {
            fs::write(&source, wrong).unwrap();
            assert!(crate::install::store_key_setting(&deploy).is_err());
        }
    }

    /// The repository's key is the Store's production key, as its keeper
    /// posted it (#3692): one line, 32 bytes, fingerprint
    /// `8717ddcdf9ac22fb`, so every build made from here carries it.
    #[test]
    fn the_repositorys_store_key_is_the_stores_production_key() {
        let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
        let setting = crate::install::store_key_setting(&deploy).unwrap().unwrap();
        assert_eq!(setting.path, crate::install::STORE_KEY_SETTING);
        assert_eq!(setting.mode.as_deref(), Some("0644"));
        let keys = manifest::read_public_keys(setting.text.as_deref().unwrap()).unwrap();
        let fingerprints: Vec<_> = keys.iter().map(manifest::fingerprint).collect();
        assert_eq!(fingerprints, ["8717ddcdf9ac22fb"]);
        let mut settings = crate::install::product_settings();
        settings.push(setting);
        manifest::check_settings(&settings).unwrap();
    }

    #[test]
    fn the_images_files_are_carried_with_the_updater_they_need() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        let deploy = folder.path().join("deploy");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&deploy).unwrap();
        fs::write(out.join("lamp-update"), b"update").unwrap();
        let lamp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../robots/lamp");
        let shipped =
            crate::image::shipped_from(include_str!("../../../deploy/image/shipped.json")).unwrap();
        let image = crate::image::from_tree(&lamp, &shipped).unwrap();
        let commit = "d".repeat(40);
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let made = make_with(
            &out,
            &deploy,
            &skills,
            "lamp-voice",
            9,
            &commit,
            "https://channel.test/build-9/",
            None,
            Vec::new(),
            Vec::new(),
            image.clone(),
            None,
        )
        .unwrap();
        assert_eq!(made.image, image);
        assert_eq!(made.min_updater, Some(manifest::IMAGE_UPDATER));
        let written = fs::read(out.join("manifest.json")).unwrap();
        assert!(
            written.len() < manifest::MOST_MANIFEST_BYTES,
            "{}",
            written.len()
        );
        assert_eq!(manifest::parse(&written).unwrap(), made);
        // An updater older than the field refuses it by min_updater, clearly.
        let too_old = manifest::too_old(&written, manifest::SETTINGS_UPDATER).unwrap();
        assert_eq!(too_old.needed, manifest::IMAGE_UPDATER);
    }

    /// Every unit a build carries for a dev lamp by name (`deploy/lamp-*/`
    /// with its LAMP.md), made from the repository's own folders, leaves
    /// that lamp's round free to take the product's units once it is a
    /// product install (#3796, #1843): each is pinned, with a product unit
    /// of its name started through `current`, so the round skips it
    /// ("skipping pinned", crate::round). Without a product unit, a lamp of
    /// that name, a dev lamp moved onto product units or a customer's lamp
    /// with the same four last digits, refuses every round (lamp-52e6 on 27
    /// Oct); and one through `current` would be carried in place of the
    /// product's unit there, with every line of its own (Reviewer-three's P3
    /// on #4352).
    #[test]
    fn a_dev_lamps_units_never_stop_a_product_install_of_its_name() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("lamp-update"), "update").unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let made = make(
            &out,
            &root.join("deploy"),
            &root.join("skills"),
            "lamp-voice",
            7,
            &"c".repeat(40),
            "https://channel.test/build-7/",
            None,
            Vec::new(),
        )
        .unwrap();
        let text = |unit: &Unit| fs::read_to_string(out.join(unit.file_name())).unwrap();
        let product: BTreeMap<&str, String> = made
            .units
            .iter()
            .filter(|unit| unit.lamp == manifest::PRODUCT_LAMP)
            .map(|unit| (unit.unit.as_str(), text(unit)))
            .collect();
        let mut hosts = BTreeMap::<&str, usize>::new();
        for unit in made
            .units
            .iter()
            .filter(|unit| unit.lamp != manifest::PRODUCT_LAMP)
        {
            *hosts.entry(unit.lamp.as_str()).or_default() += 1;
            let own = text(unit);
            let theirs = product
                .get(unit.unit.as_str())
                .and_then(|text| unit::linked(text));
            let pinned = unit::program(&own);
            assert!(
                theirs.is_some() && pinned.is_some(),
                "{}'s {}: the product's {:?} through current, the lamp's pinned {:?} (through current {:?})",
                unit.lamp,
                unit.unit,
                theirs,
                pinned,
                unit::linked(&own)
            );
        }
        // The two dev lamps whose units moved 52e6 back on 27 Oct are read.
        assert_eq!(hosts.get("lamp-52e6"), Some(&12), "{hosts:?}");
        assert_eq!(hosts.get("lamp-4ace"), Some(&16), "{hosts:?}");
    }
}
