//! The manifest of a build, made where it is built: every lampOS binary
//! in a folder (CI's `arm64-out`, with its `SHA256SUMS`), every unit of
//! every lamp's own folder (`deploy/<lamp>/`, a folder with a `LAMP.md`),
//! and the product's skills (`skills/`, each of
//! [`PRODUCT_SKILLS`](crate::install::PRODUCT_SKILLS)) as one file,
//! [`product_skills::BUNDLE`]. The units are copied beside the binaries as
//! `<lamp>.<unit>`, and the skills written there, so that the folder is
//! what is published.
use crate::{
    install,
    manifest::{self, File, Manifest, Model, SCHEMA, Setting, Unit},
    product_skills, unit,
};
use std::{fs, io, path::Path};

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
    )
}

/// [`make`], with the product's `settings` carried too (#1680):
/// `min_updater` is then at least [`manifest::SETTINGS_UPDATER`].
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
    let mut units = Vec::new();
    for lamp in fs::read_dir(deploy)? {
        let lamp = lamp?;
        let name = lamp.file_name().to_string_lossy().into_owned();
        if !manifest::is_token(&name) || !lamp.path().join("LAMP.md").is_file() {
            continue;
        }
        for entry in fs::read_dir(lamp.path())? {
            let entry = entry?;
            let unit_name = entry.file_name().to_string_lossy().into_owned();
            if !manifest::is_unit_name(&unit_name) || !entry.file_type()?.is_file() {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            // Only a part installed by build: the others are not updated.
            let Some(_) = std::str::from_utf8(&bytes).ok().and_then(unit::program) else {
                continue;
            };
            let unit = Unit {
                lamp: name.clone(),
                unit: unit_name,
                sha256: manifest::sha256_hex(&bytes),
                size: bytes.len() as u64,
            };
            fs::write(binaries.join(unit.file_name()), &bytes)?;
            units.push(unit);
        }
    }
    units.sort_by(|a, b| (&a.lamp, &a.unit).cmp(&(&b.lamp, &b.unit)));
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
        )
        .unwrap();
        assert_eq!(made.settings, settings);
        assert_eq!(made.min_updater, Some(manifest::SETTINGS_UPDATER));
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
}
