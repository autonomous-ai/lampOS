//! Core and installable skills (`docs/skills.md`): the package the Skills
//! Store will carry, checked file by file, and an installable skill put on,
//! replaced and taken off a skills folder, a core one never.
use lamp_skills::{
    SkillKind, load,
    package::{self, Entry, MANIFEST, Manifest, SIGNATURE, read_folder, read_package, sha256_hex},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The repository's skill `name` copied into `dir`, `from` in its
/// `skill.toml` made `to`.
fn copied(dir: &Path, name: &str, from: &str, to: &str) -> PathBuf {
    let skill = dir.join(name);
    copy(&skills_root().join(name), &skill);
    let toml = fs::read_to_string(skill.join("skill.toml")).unwrap();
    assert!(toml.contains(from), "{name}: {from}");
    fs::write(skill.join("skill.toml"), toml.replacen(from, to, 1)).unwrap();
    skill
}

/// Desk tutor, installable, at `version`, in `dir`.
fn tutor(dir: &Path, version: &str) -> PathBuf {
    copied(
        dir,
        "desk-tutor",
        "version = \"1.0.0\"",
        &format!("version = \"{version}\""),
    )
}

/// Every name in a skills folder, hidden ones too: what an install left.
fn left(skills: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(skills)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A plain file's header in a tar, its name written as given, unchecked:
/// a package made by hand, or by someone who means harm.
fn header(path: &str, size: usize, kind: tar::EntryType) -> tar::Header {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(kind);
    header.set_size(size as u64);
    header.set_mode(0o644);
    header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
    header.set_cksum();
    header
}

/// A tar of these entries, in order, each a plain file.
fn tar_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let header = header(path, bytes.len(), tar::EntryType::Regular);
        builder.append(&header, &bytes[..]).unwrap();
    }
    builder.into_inner().unwrap()
}

/// The manifest of `files` for `name` at `version`.
fn manifest(name: &str, version: &str, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    serde_json::to_vec(&Manifest {
        format: package::FORMAT,
        name: name.into(),
        version: version.into(),
        files: files
            .iter()
            .map(|(path, bytes)| Entry {
                path: path.clone(),
                size: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
    })
    .unwrap()
}

/// Desk tutor's files, by their path in its folder.
fn tutor_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    read_folder(&tutor(dir, "1.0.0"))
        .unwrap()
        .into_iter()
        .collect()
}

/// A package of `name` with this manifest and these files under its
/// folder, and anything `more` beside them, written to `dir/NAME.tar`.
fn package_of(
    dir: &Path,
    manifest: Vec<u8>,
    files: &[(String, Vec<u8>)],
    more: &[(&str, Vec<u8>)],
) -> PathBuf {
    let mut entries: Vec<(String, Vec<u8>)> = vec![(MANIFEST.into(), manifest)];
    entries.extend(
        files
            .iter()
            .map(|(path, bytes)| (format!("desk-tutor/{path}"), bytes.clone())),
    );
    entries.extend(
        more.iter()
            .map(|(path, bytes)| ((*path).to_owned(), bytes.clone())),
    );
    let entries: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.clone()))
        .collect();
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("desk-tutor.tar");
    fs::write(&path, tar_of(&entries)).unwrap();
    path
}

#[test]
fn a_skill_packs_to_a_versioned_tar_of_its_files_and_their_sha256_the_same_each_time() {
    let dir = tempfile::tempdir().unwrap();
    let skill = tutor(dir.path(), "1.2.3");
    // A Mac's AppleDouble file is not the skill's, and not packed.
    fs::write(skill.join("._SKILL.md"), b"\x00\x05\x16\x07").unwrap();
    let path = package::pack(&skill, &dir.path().join("out")).unwrap();
    assert_eq!(path.file_name().unwrap(), "desk-tutor-1.2.3.tar");
    let read = read_package(&path).unwrap();
    assert_eq!(read.manifest.format, 1);
    assert_eq!(read.manifest.name, "desk-tutor");
    assert_eq!(read.manifest.version, "1.2.3");
    assert!(read.signature.is_none());
    let on_disk = read_folder(&skill).unwrap();
    assert!(!on_disk.contains_key("._SKILL.md"));
    assert!(on_disk.contains_key("SKILL.md") && on_disk.contains_key("skill.toml"));
    assert_eq!(read.files, on_disk);
    for entry in &read.manifest.files {
        let bytes = fs::read(skill.join(&entry.path)).unwrap();
        assert_eq!(entry.sha256, sha256_hex(&bytes), "{}", entry.path);
        assert_eq!(entry.size, bytes.len() as u64);
    }
    // The manifest first, then the skill's folder; no time, no owner.
    let bytes = fs::read(&path).unwrap();
    let mut archive = tar::Archive::new(&bytes[..]);
    let paths: Vec<String> = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert_eq!(entry.header().mtime().unwrap(), 0);
            assert_eq!(entry.header().uid().unwrap(), 0);
            entry.path().unwrap().to_string_lossy().into_owned()
        })
        .collect();
    assert_eq!(paths[0], MANIFEST);
    assert!(
        paths[1..]
            .iter()
            .all(|path| path.starts_with("desk-tutor/"))
    );
    // The same skill packs to the same bytes.
    let again = package::pack(&skill, &dir.path().join("again")).unwrap();
    assert_eq!(fs::read(again).unwrap(), bytes);
}

#[test]
fn a_core_skill_or_one_without_a_version_or_that_does_not_load_is_not_packed() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let dance = copied(dir.path(), "dance", "", "");
    let why = package::pack(&dance, &out).unwrap_err();
    assert!(why.contains("dance is a core skill"), "{why}");
    let unversioned = copied(
        &dir.path().join("plain"),
        "desk-tutor",
        "version = \"1.0.0\"\n",
        "",
    );
    let why = package::pack(&unversioned, &out).unwrap_err();
    assert!(why.contains("says no version"), "{why}");
    let unlit = copied(
        &dir.path().join("unlit"),
        "desk-tutor",
        "light = true\n",
        "",
    );
    let why = package::pack(&unlit, &out).unwrap_err();
    assert!(
        why.contains("not permitted: light needs light = true"),
        "{why}"
    );
    assert!(!out.exists() || left(&out).is_empty());
}

#[test]
fn a_package_installs_whole_its_files_checked_and_open_to_the_parts() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let source = tutor(&dir.path().join("src"), "1.0.0");
    // Closed on the computer it came from: open on the lamp all the same.
    fs::set_permissions(source.join("SKILL.md"), fs::Permissions::from_mode(0o600)).unwrap();
    let path = package::pack(&source, dir.path()).unwrap();
    let said = package::install(&skills, &path).unwrap();
    assert!(
        said.starts_with("installed desk-tutor 1.0.0 (installable) from its package"),
        "{said}"
    );
    assert!(
        said.contains("each one's size and sha256 as lamp-skill.json says"),
        "{said}"
    );
    assert!(
        said.contains("it carries no signature: a package installed by hand from a file"),
        "{said}"
    );
    assert!(
        said.contains("It may use: speak, light, body, camera."),
        "{said}"
    );
    // What its function costs a session is said, not left to be found.
    assert!(
        said.ends_with(
            "A running lamp-skills loads it within 2 s. The skills here give the model 1 function \
             (check_my_work). A session takes 32, the voice's own and the actions part's first \
             (30 on a product lamp), and past 32 the last are dropped unless the voice runs with \
             LAMP_USE_SKILL=on (every skill through one function, use_skill)."
        ),
        "{said}"
    );
    // Only the skill: nothing put together or set aside is left beside it.
    assert_eq!(left(&skills), ["desk-tutor"]);
    let installed = skills.join("desk-tutor");
    assert_eq!(
        read_folder(&installed).unwrap(),
        read_folder(&source).unwrap()
    );
    let skill = load(&installed).unwrap();
    assert_eq!(skill.kind, SkillKind::Installable);
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&installed), 0o755);
    assert_eq!(mode(&installed.join("SKILL.md")), 0o644);
    assert_eq!(mode(&installed.join("tests")), 0o755);
}

#[test]
fn a_package_changed_damaged_or_out_of_shape_installs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let files = tutor_files(&dir.path().join("src"));
    let good = manifest("desk-tutor", "1.0.0", &files);
    // One letter of the description changed, the size the same: only the
    // SHA-256 tells. And one byte more, which the size tells.
    let changed_by = |change: fn(&mut Vec<u8>)| -> Vec<(String, Vec<u8>)> {
        files
            .iter()
            .map(|(path, bytes)| {
                let mut bytes = bytes.clone();
                if path == "SKILL.md" {
                    change(&mut bytes);
                }
                (path.clone(), bytes)
            })
            .collect()
    };
    let changed = changed_by(|bytes| {
        let at = bytes.len() / 2;
        bytes[at] ^= 0x20;
    });
    let longer = changed_by(|bytes| bytes.push(b'!'));
    let without_toml: Vec<(String, Vec<u8>)> = files
        .iter()
        .filter(|(path, _)| path != "skill.toml")
        .cloned()
        .collect();
    let mut format_two: serde_json::Value = serde_json::from_slice(&good).unwrap();
    format_two["format"] = 2.into();
    let unlit: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, bytes)| {
            let text = String::from_utf8(bytes.clone()).unwrap_or_default();
            if path == "skill.toml" {
                (
                    path.clone(),
                    text.replacen("light = true\n", "", 1).into_bytes(),
                )
            } else {
                (path.clone(), bytes.clone())
            }
        })
        .collect();
    let link = {
        let mut builder = tar::Builder::new(Vec::new());
        builder
            .append(
                &header(MANIFEST, good.len(), tar::EntryType::Regular),
                &good[..],
            )
            .unwrap();
        let mut link = header("desk-tutor/SKILL.md", 0, tar::EntryType::Symlink);
        link.set_link_name("/etc/hostname").unwrap();
        link.set_cksum();
        builder.append(&link, &[][..]).unwrap();
        builder.into_inner().unwrap()
    };
    let cases: Vec<(&str, PathBuf, &str)> = vec![
        (
            "changed",
            package_of(&dir.path().join("changed"), good.clone(), &changed, &[]),
            "SKILL.md: its size or sha256 is not what lamp-skill.json says",
        ),
        (
            "longer",
            package_of(&dir.path().join("longer"), good.clone(), &longer, &[]),
            "SKILL.md: its size or sha256 is not what lamp-skill.json says",
        ),
        (
            "extra",
            package_of(
                &dir.path().join("extra"),
                good.clone(),
                &files,
                &[("desk-tutor/run.sh", b"rm -rf /".to_vec())],
            ),
            "run.sh is in the package and not in lamp-skill.json",
        ),
        (
            "missing",
            package_of(
                &dir.path().join("missing"),
                good.clone(),
                &without_toml,
                &[],
            ),
            "skill.toml is in lamp-skill.json and not in the package",
        ),
        (
            "up",
            package_of(
                &dir.path().join("up"),
                good.clone(),
                &files,
                &[("desk-tutor/../outside", b"x".to_vec())],
            ),
            "a skill's paths are",
        ),
        (
            "hidden",
            package_of(
                &dir.path().join("hidden"),
                good.clone(),
                &files,
                &[("desk-tutor/._SKILL.md", b"x".to_vec())],
            ),
            "none beginning with a dot",
        ),
        (
            "two",
            package_of(
                &dir.path().join("two"),
                good.clone(),
                &files,
                &[("other/SKILL.md", b"x".to_vec())],
            ),
            "a package holds one skill",
        ),
        (
            "beside",
            package_of(
                &dir.path().join("beside"),
                good.clone(),
                &files,
                &[("install.sh", b"x".to_vec())],
            ),
            "beside the skill's folder",
        ),
        (
            "format",
            package_of(
                &dir.path().join("format"),
                serde_json::to_vec(&format_two).unwrap(),
                &files,
                &[],
            ),
            "format 2 is not one this lamp reads",
        ),
        (
            "renamed",
            package_of(
                &dir.path().join("renamed"),
                manifest("desk-helper", "1.0.0", &files),
                &files,
                &[],
            ),
            "names \"desk-helper\", and the package's folder is \"desk-tutor\"",
        ),
        (
            "version",
            package_of(
                &dir.path().join("version"),
                manifest("desk-tutor", "2.0.0", &files),
                &files,
                &[],
            ),
            "lamp-skill.json says version 2.0.0, and its skill.toml 1.0.0",
        ),
        (
            "unlit",
            package_of(
                &dir.path().join("unlit"),
                manifest("desk-tutor", "1.0.0", &unlit),
                &unlit,
                &[],
            ),
            "does not load: not permitted: light needs light = true",
        ),
        (
            "link",
            {
                let path = dir.path().join("link.tar");
                fs::write(&path, link).unwrap();
                path
            },
            "a package holds plain files",
        ),
        (
            "no manifest",
            {
                let path = dir.path().join("bare.tar");
                fs::write(&path, tar_of(&[("desk-tutor/SKILL.md", b"x".to_vec())])).unwrap();
                path
            },
            "no lamp-skill.json: not a skill's package",
        ),
    ];
    for (case, path, why) in cases {
        let error = package::install(&skills, &path).unwrap_err();
        assert!(error.contains(why), "{case}: {error}");
        assert!(error.starts_with("not installed"), "{case}: {error}");
        assert!(left(&skills).is_empty(), "{case}: {:?}", left(&skills));
    }
    // The same files and manifest, untouched, do install.
    let path = package_of(&dir.path().join("good"), good, &files, &[]);
    package::install(&skills, &path).unwrap();
    assert_eq!(left(&skills), ["desk-tutor"]);
}

#[test]
fn a_packages_signature_is_carried_and_not_checked() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let files = tutor_files(&dir.path().join("src"));
    let path = package_of(
        &dir.path().join("signed"),
        manifest("desk-tutor", "1.0.0", &files),
        &files,
        &[(SIGNATURE, b"not a signature at all".to_vec())],
    );
    assert_eq!(
        read_package(&path).unwrap().signature.as_deref(),
        Some(&b"not a signature at all"[..])
    );
    let said = package::install(&skills, &path).unwrap();
    assert!(
        said.contains("its signature not checked: a package installed by hand from a file"),
        "{said}"
    );
}

#[test]
fn an_installable_skill_is_replaced_by_its_next_version_and_removed_whole() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let first = tutor(&dir.path().join("one"), "1.0.0");
    let said = package::install(&skills, &first).unwrap();
    assert!(
        said.starts_with("installed desk-tutor 1.0.0 (installable) from its folder"),
        "{said}"
    );
    let next = package::pack(&tutor(&dir.path().join("two"), "1.1.0"), dir.path()).unwrap();
    let said = package::install(&skills, &next).unwrap();
    assert!(
        said.starts_with("replaced desk-tutor 1.0.0 with 1.1.0 (installable)"),
        "{said}"
    );
    assert_eq!(left(&skills), ["desk-tutor"]);
    assert_eq!(
        load(&skills.join("desk-tutor")).unwrap().version.as_deref(),
        Some("1.1.0")
    );
    let said = package::remove(&skills, "desk-tutor").unwrap();
    assert!(said.starts_with("removed desk-tutor"), "{said}");
    assert!(left(&skills).is_empty());
    let why = package::remove(&skills, "desk-tutor").unwrap_err();
    assert_eq!(why, "no skill desk-tutor on this lamp");
    let why = package::remove(&skills, "../skills").unwrap_err();
    assert!(why.contains("is not a skill's name"), "{why}");
}

#[test]
fn a_skill_put_on_from_a_package_keeps_its_sha256_and_manifest_and_one_from_a_folder_none() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let installed = skills.join("desk-tutor");
    // The catalogue's sha256 is the package's own bytes'.
    let first = package::pack(&tutor(&dir.path().join("one"), "1.0.0"), dir.path()).unwrap();
    package::install(&skills, &first).unwrap();
    let kept = package::installed_from(&installed).unwrap();
    assert_eq!(kept.sha256, sha256_hex(&fs::read(&first).unwrap()));
    assert_eq!(kept.manifest, read_package(&first).unwrap().manifest);
    assert_eq!(kept.manifest.version, "1.0.0");
    // Kept in the folder, hidden: not one of the skill's files, so it is
    // never packed again, and the skill loads as before.
    assert!(installed.join(package::INSTALLED_FROM).is_file());
    assert!(
        !read_folder(&installed)
            .unwrap()
            .contains_key(package::INSTALLED_FROM)
    );
    assert!(load(&installed).is_ok());
    let mode = fs::metadata(installed.join(package::INSTALLED_FROM))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);

    // The next version replaces it, with the new package's.
    let next = package::pack(&tutor(&dir.path().join("two"), "1.1.0"), dir.path()).unwrap();
    package::install(&skills, &next).unwrap();
    let kept = package::installed_from(&installed).unwrap();
    assert_eq!(kept.sha256, sha256_hex(&fs::read(&next).unwrap()));
    assert_ne!(kept.sha256, sha256_hex(&fs::read(&first).unwrap()));
    assert_eq!(kept.manifest.version, "1.1.0");

    // Its version changed by hand: no longer that package.
    let toml = fs::read_to_string(installed.join("skill.toml")).unwrap();
    fs::write(
        installed.join("skill.toml"),
        toml.replacen("version = \"1.1.0\"", "version = \"1.2.0\"", 1),
    )
    .unwrap();
    assert_eq!(package::installed_from(&installed), None);

    // Put on from a folder, even the installed one's copy: none.
    let copy = dir.path().join("copy").join("desk-tutor");
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    fs::rename(&installed, &copy).unwrap();
    package::install(&skills, &copy).unwrap();
    assert!(!installed.join(package::INSTALLED_FROM).exists());
    assert_eq!(package::installed_from(&installed), None);

    // Removed: gone with the skill.
    package::install(&skills, &next).unwrap();
    assert!(package::installed_from(&installed).is_some());
    package::remove(&skills, "desk-tutor").unwrap();
    assert!(left(&skills).is_empty());

    // A package can't carry a record of its own: a hidden path is refused.
    let files = tutor_files(&dir.path().join("src"));
    let mut forged = files.clone();
    forged.push((package::INSTALLED_FROM.into(), b"{}".to_vec()));
    let path = package_of(
        &dir.path().join("forged"),
        manifest("desk-tutor", "1.0.0", &forged),
        &forged,
        &[],
    );
    let why = package::install(&skills, &path).unwrap_err();
    assert!(why.contains("none beginning with a dot"), "{why}");

    // A core skill, as it comes with the lamp, has none.
    assert_eq!(package::installed_from(&skills_root().join("dance")), None);
}

#[test]
fn a_core_skill_is_never_installed_replaced_or_removed() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    // A core skill comes with the lamp, never from a folder or a package.
    let dance = copied(&dir.path().join("src"), "dance", "", "");
    let why = package::install(&skills, &dance).unwrap_err();
    assert!(why.contains("dance says kind = \"core\""), "{why}");
    assert!(left(&skills).is_empty());
    // As lamp-update install puts it.
    copy(
        &skills_root().join("welcome-back"),
        &skills.join("welcome-back"),
    );
    let before = read_folder(&skills.join("welcome-back")).unwrap();
    let theirs = copied(
        &dir.path().join("theirs"),
        "welcome-back",
        "kind = \"core\"",
        "kind = \"installable\"",
    );
    let why = package::install(&skills, &theirs).unwrap_err();
    assert!(
        why.contains("welcome-back is a core skill on this lamp"),
        "{why}"
    );
    let why = package::remove(&skills, "welcome-back").unwrap_err();
    assert_eq!(
        why,
        "welcome-back is a core skill: it comes with the lamp and can't be removed"
    );
    assert_eq!(left(&skills), ["welcome-back"]);
    assert_eq!(read_folder(&skills.join("welcome-back")).unwrap(), before);
}

#[test]
fn a_disabled_core_stays_in_inventory_and_cannot_be_packaged_replaced_or_removed() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let core = copied(&dir.path().join("source"), "bedtime-morning", "", "");
    assert!(!load(&core).unwrap().enabled);
    assert!(
        package::pack(&core, &dir.path().join("out"))
            .unwrap_err()
            .contains("core skill")
    );
    assert!(
        package::install(&skills, &core)
            .unwrap_err()
            .contains("says kind = \"core\"")
    );
    copy(&core, &skills.join("bedtime-morning"));
    let before = read_folder(&skills.join("bedtime-morning")).unwrap();
    let listed = package::list(&skills).unwrap();
    let row = listed
        .lines()
        .find(|line| line.starts_with("bedtime-morning "))
        .unwrap();
    assert_eq!(
        row.split_whitespace().take(4).collect::<Vec<_>>(),
        ["bedtime-morning", "core", "1.0.0", "disabled;"]
    );
    assert!(listed.ends_with("The skills here give the model no function.\n"));
    assert_eq!(lamp_skills::load_each(&skills).len(), 1);
    let (live, errors) = lamp_skills::load_all(&skills);
    assert!(live.is_empty());
    assert!(errors.is_empty());
    let replacement = copied(
        &dir.path().join("replacement"),
        "bedtime-morning",
        "kind = \"core\"",
        "kind = \"installable\"",
    );
    assert!(
        package::install(&skills, &replacement)
            .unwrap_err()
            .contains("is a core skill on this lamp")
    );
    assert!(
        package::remove(&skills, "bedtime-morning")
            .unwrap_err()
            .contains("can't be removed")
    );
    assert_eq!(
        read_folder(&skills.join("bedtime-morning")).unwrap(),
        before
    );
}

#[test]
fn a_disabled_installable_reserves_its_tool_but_adds_no_live_function() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let disabled = copied(
        &skills,
        "desk-tutor",
        "kind = \"installable\"",
        "enabled = false\nkind = \"installable\"",
    );
    let listed = package::list(&skills).unwrap();
    assert!(
        listed
            .lines()
            .any(|line| line.starts_with("desk-tutor ") && line.contains("disabled;"))
    );
    assert!(listed.ends_with("The skills here give the model no function.\n"));
    let (live, errors) = lamp_skills::load_all(&skills);
    assert!(live.is_empty());
    assert!(errors.is_empty());

    let duplicate = dir.path().join("source/tutor-copy");
    fs::create_dir_all(&duplicate).unwrap();
    fs::write(
        duplicate.join("SKILL.md"),
        "---\nname: tutor-copy\ndescription: A tool reservation check.\n---\n",
    )
    .unwrap();
    fs::write(
        duplicate.join("skill.toml"),
        "tool = \"check_my_work\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Ready.\"\n",
    )
    .unwrap();
    let why = package::install(&skills, &duplicate).unwrap_err();
    assert!(why.contains("the installable skill desk-tutor's"), "{why}");

    // Disabling does not hide validation failures, and an invalid installed
    // definition still reserves its raw-manifest tool for install purposes.
    let path = disabled.join("skill.toml");
    let text = fs::read_to_string(&path).unwrap();
    // Revoke the grant alone; the later [[do]] still asks for a look.
    fs::write(&path, text.replacen("camera = true", "camera = false", 1)).unwrap();
    let why = load(&disabled).unwrap_err();
    assert!(why.contains("needs camera = true"), "{why}");
    let listed = package::list(&skills).unwrap();
    assert!(listed.contains("not loaded: not permitted: look needs camera = true"));
    assert!(
        package::install(&skills, &duplicate)
            .unwrap_err()
            .contains("the installable skill desk-tutor's")
    );
    package::remove(&skills, "desk-tutor").unwrap();
    assert!(left(&skills).is_empty());
}

/// The reviewer's probe on #1698 (08:39): an installable `a-dance` with
/// `tool = "dance"` installed beside core Dance and took its function.
/// A function the model calls is one skill's: an install whose tool's name
/// another skill on the lamp has is refused, a core one's or not; the same
/// skill's next version keeps its own.
#[test]
fn a_skill_whose_tool_another_skill_has_is_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    copy(&skills_root().join("dance"), &skills.join("dance"));
    let skill_of = |name: &str, tool: &str| {
        let skill = dir.path().join("src").join(name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: When they ask for it.\n---\n"),
        )
        .unwrap();
        fs::write(
            skill.join("skill.toml"),
            format!(
                "tool = \"{tool}\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n"
            ),
        )
        .unwrap();
        skill
    };
    let why = package::install(&skills, &skill_of("a-dance", "dance")).unwrap_err();
    assert_eq!(
        why,
        "not installed: a-dance's tool \"dance\" is the core skill dance's on this lamp: \
         a function the model calls is one skill's"
    );
    assert_eq!(left(&skills), ["dance"]);
    let said = package::install(&skills, &tutor(&dir.path().join("one"), "1.0.0")).unwrap();
    assert!(
        said.ends_with(
            "The skills here give the model 2 functions (dance, check_my_work). A session takes 32, \
             the voice's own and the actions part's first (30 on a product lamp), and past 32 the \
             last are dropped unless the voice runs with LAMP_USE_SKILL=on (every skill through \
             one function, use_skill)."
        ),
        "{said}"
    );
    let why = package::install(&skills, &skill_of("tutor-two", "check_my_work")).unwrap_err();
    assert!(
        why.contains("tool \"check_my_work\" is the installable skill desk-tutor's on this lamp"),
        "{why}"
    );
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    let said = package::install(&skills, &tutor(&dir.path().join("two"), "1.1.0")).unwrap();
    assert!(
        said.starts_with("replaced desk-tutor 1.0.0 with 1.1.0"),
        "{said}"
    );
}

/// A killed install or remove leaves its `.install-NAME-PID` or
/// `.removed-NAME-PID` (a whole skill), which no later run of another PID
/// would delete (the reviewer on #1698, 08:39). The next install deletes
/// them and says so; nothing else in the folder of a name like them is
/// touched, and a link of that name is not followed.
#[test]
fn what_a_killed_install_or_remove_left_is_deleted_by_the_next_install() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let staged = skills.join(".install-desk-tutor-99999");
    tutor(&staged, "1.0.0");
    let aside = skills.join(".removed-old-skill-12");
    fs::create_dir_all(aside.join("tests")).unwrap();
    fs::write(aside.join("SKILL.md"), b"an old skill").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("keep.txt"), b"not the skills'").unwrap();
    std::os::unix::fs::symlink(&elsewhere, skills.join(".removed-x-12")).unwrap();
    fs::write(skills.join(".install-y-7"), b"a file").unwrap();
    for kept in [".install-notes", ".installer-x-1", ".install-z-1a"] {
        fs::create_dir(skills.join(kept)).unwrap();
    }
    let said = package::install(&skills, &tutor(&dir.path().join("src"), "1.0.0")).unwrap();
    assert!(
        said.ends_with(
            "Deleted what a killed install or remove left: .install-desk-tutor-99999, .removed-old-skill-12."
        ),
        "{said}"
    );
    let mut kept = vec![
        ".install-notes",
        ".install-y-7",
        ".install-z-1a",
        ".installer-x-1",
        ".removed-x-12",
        "desk-tutor",
    ];
    kept.sort();
    assert_eq!(left(&skills), kept);
    assert_eq!(
        fs::read(elsewhere.join("keep.txt")).unwrap(),
        b"not the skills'"
    );
    // Nothing left this time, and nothing said of it.
    let said = package::install(&skills, &tutor(&dir.path().join("next"), "1.1.0")).unwrap();
    assert!(!said.contains("Deleted"), "{said}");
}

/// An install holds the skills folder for its whole run, so another
/// install or remove waits for it, and what an install takes for a killed
/// run's is never one under way.
#[test]
fn an_install_waits_while_another_run_holds_the_skills_folder() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let source = tutor(&dir.path().join("src"), "1.0.0");
    let held = fs::File::open(&skills).unwrap();
    held.lock().unwrap();
    let (done, finished) = std::sync::mpsc::channel();
    let installing = {
        let skills = skills.clone();
        std::thread::spawn(move || done.send(package::install(&skills, &source)).unwrap())
    };
    assert!(
        finished
            .recv_timeout(std::time::Duration::from_millis(300))
            .is_err()
    );
    assert!(left(&skills).is_empty());
    held.unlock().unwrap();
    let said = finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert!(said.starts_with("installed desk-tutor 1.0.0"), "{said}");
    installing.join().unwrap();
}

#[test]
fn a_folder_with_a_link_or_nowhere_to_go_is_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let source = tutor(&dir.path().join("src"), "1.0.0");
    std::os::unix::fs::symlink("/etc/passwd", source.join("lights.toml")).unwrap();
    let why = package::install(&skills, &source).unwrap_err();
    assert!(why.contains("lights.toml: a link"), "{why}");
    fs::remove_file(source.join("lights.toml")).unwrap();
    let why = package::install(&dir.path().join("none"), &source).unwrap_err();
    assert!(why.starts_with("no skills folder at"), "{why}");
    let why = package::install(&skills, &dir.path().join("gone")).unwrap_err();
    assert!(why.contains("gone"), "{why}");
    assert!(left(&skills).is_empty());
}

#[test]
fn a_list_says_each_skills_name_kind_version_and_what_it_may_use() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    copy(
        &skills_root().join("welcome-back"),
        &skills.join("welcome-back"),
    );
    copy(
        &skills_root().join("desk-tutor"),
        &skills.join("desk-tutor"),
    );
    copied(&skills, "stuffy-air", "body = true\n", "");
    fs::create_dir(skills.join(".install-x-1")).unwrap();
    let listed = package::list(&skills).unwrap();
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 5, "{listed}");
    // Then what the skills that loaded give the model, against a session's room.
    assert!(
        lines[4].starts_with(
            "The skills here give the model 1 function (check_my_work). A session takes 32, \
             the voice's own and the actions part's first (30 on a product lamp), and past 32 \
             the last are dropped unless the voice runs with LAMP_USE_SKILL=on"
        ),
        "{listed}"
    );
    // Name, kind and version are single words; what it may use is the rest.
    let columns = |line: &str| {
        let mut words = line.split_whitespace();
        let mut columns: Vec<String> = words.by_ref().take(3).map(str::to_owned).collect();
        columns.push(words.collect::<Vec<_>>().join(" "));
        columns
    };
    assert_eq!(columns(lines[0]), ["NAME", "KIND", "VERSION", "MAY USE"]);
    assert_eq!(
        columns(lines[1]),
        [
            "desk-tutor",
            "installable",
            "1.0.0",
            "speak, light, body, camera"
        ]
    );
    assert_eq!(&columns(lines[2])[..3], ["stuffy-air", "core", "1.0.0"]);
    assert!(
        columns(lines[2])[3]
            .starts_with("not loaded: not permitted: body.gesture needs body = true"),
        "{listed}"
    );
    assert_eq!(
        columns(lines[3]),
        [
            "welcome-back",
            "core",
            "1.0.0",
            "reads humans/faces/tracked, voice/state; speak, light, body, who is at the lamp"
        ]
    );
}

/// The manage helper as systemd runs it for a connection to its socket
/// (`lamp-skills-manage.socket`, `Accept=yes`): the real program, `lamp-skills
/// asked`, its standard input and output one end of a Unix socket, asked
/// `line` straight, with nobody before it to check the request (#1722).
fn ask_helper(skills: &Path, line: &[u8]) -> (serde_json::Value, bool) {
    let (answer, ok, _) = ask_helper_journal(skills, line);
    (answer, ok)
}

/// [`ask_helper`], with what the helper wrote to its journal (its standard
/// error) as well.
fn ask_helper_journal(skills: &Path, line: &[u8]) -> (serde_json::Value, bool, String) {
    ask_helper_while(skills, line, |_| {})
}

/// [`ask_helper_journal`], `meanwhile` given the running helper once its
/// request is written, before its answer is read.
fn ask_helper_while(
    skills: &Path,
    line: &[u8],
    meanwhile: impl FnOnce(&mut std::process::Child),
) -> (serde_json::Value, bool, String) {
    let (answer, ok, journal, _) = ask_helper_as(skills, line, true, |_| {}, meanwhile);
    (answer, ok, journal)
}

struct RequestOutcome {
    write: std::io::Result<()>,
    half_close: std::io::Result<()>,
}

/// Reap the exact helper even if a fixture check or callback panics.
struct HelperChild(std::process::Child);

impl Drop for HelperChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// [`ask_helper_while`], the peer let through by the helper's test switch
/// (`package::ANY_ASKER`) only when `any_asker`. The early-refusal control
/// can observe the child's exit before the request is written.
fn ask_helper_as(
    skills: &Path,
    line: &[u8],
    any_asker: bool,
    before_request: impl FnOnce(&mut std::process::Child),
    meanwhile: impl FnOnce(&mut std::process::Child),
) -> (serde_json::Value, bool, String, RequestOutcome) {
    use std::io::{BufRead, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    let (mut ours, theirs) = UnixStream::pair().unwrap();
    let mut child = HelperChild(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args(["asked", "--dir"])
            .env(package::ANY_ASKER, if any_asker { "on" } else { "" })
            .arg(skills)
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(
                theirs.try_clone().unwrap(),
            )))
            .stdout(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let child = &mut child.0;
    before_request(child);
    let write = ours.write_all(line);
    if any_asker {
        // Authorized requests must still be delivered before the callback.
        write.as_ref().unwrap();
    }
    let half_close = ours.shutdown(std::net::Shutdown::Write);
    if any_asker {
        half_close.as_ref().unwrap();
    }
    let request = RequestOutcome { write, half_close };
    meanwhile(child);
    let mut answer = String::new();
    std::io::BufReader::new(&ours)
        .read_line(&mut answer)
        .unwrap();
    let mut journal = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut journal)
        .unwrap();
    let status = child.wait().unwrap();
    (
        serde_json::from_str(&answer).unwrap(),
        status.success(),
        journal,
        request,
    )
}

/// A peer that is neither `lamp-cloud` nor the voice (here the test itself,
/// on a socket pair) is refused before its line is read, and the skill
/// stays (the reviewer's P3 on #2429).
#[test]
fn a_peer_that_is_not_an_asker_is_refused_and_nothing_is_removed() {
    check_non_asker_refusal(false);
}

/// Make the early refusal precede the write, without a scheduling guess.
#[test]
fn a_peer_refused_before_its_request_is_written_still_leaves_the_skill_in_place() {
    check_non_asker_refusal(true);
}

fn check_non_asker_refusal(after_exit: bool) {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    tutor(&skills, "1.0.0");
    let (answer, ok, journal, request) = ask_helper_as(
        &skills,
        b"{\"remove\": \"desk-tutor\"}\n",
        false,
        |child| {
            if after_exit {
                wait_for_helper_exit(child);
            }
        },
        |_| {},
    );
    // Collect all three product observations before judging the write.
    let preserved = skills.join("desk-tutor").join("skill.toml").is_file();
    assert!(!ok, "{answer}");
    assert_eq!(
        answer,
        serde_json::json!({"refused": package::NOT_AN_ASKER})
    );
    assert!(preserved);
    let broken_pipe = request
        .write
        .as_ref()
        .is_err_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe);
    assert!(
        request.write.is_ok() || broken_pipe,
        "request write: {:?}; {answer}; {journal}",
        request.write
    );
    assert!(
        request.half_close.is_ok()
            || (broken_pipe
                && request
                    .half_close
                    .as_ref()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotConnected)),
        "request half-close: {:?}; {answer}; {journal}",
        request.half_close
    );
    if after_exit {
        assert!(
            broken_pipe,
            "the exited helper must refuse the request write"
        );
    }
}

fn wait_for_helper_exit(child: &mut std::process::Child) {
    use std::time::{Duration, Instant};
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the helper did not exit before the request write"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// While another run holds the skills folder, the helper waits at most
/// its lock wait (`ASKED_LOCK_WAIT`, under `lamp-cloud`'s 5 s, on the lamp;
/// 200 ms here, so the test does not wait the lamp's 4 s out, #1806), then
/// refuses in the words the app already shows, with nothing done, even
/// once the folder is given back; a hold given back within it lets the
/// remove go on (the reviewer's P3 1 on #1890). That half asks the binary,
/// which waits the lamp's 4 s, and gives the folder back only 300 ms after
/// the binary is seen with it open, which it opens only to wait for its
/// lock: a binary that did not wait has refused by then, however late it
/// started (a first start takes up to 5 s on a Mac, #2348).
#[test]
fn the_manage_helper_waits_for_a_held_folder_only_so_long_then_does_nothing() {
    use std::time::{Duration, Instant};
    const WAIT: Duration = Duration::from_millis(200);
    const _: () = assert!(package::ASKED_LOCK_WAIT.as_millis() < 5_000);
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    tutor(&skills, "1.0.0");
    let held = fs::File::open(&skills).unwrap();
    held.lock().unwrap();
    let asked = Instant::now();
    let mut answer = Vec::new();
    let ok = package::asked(
        &skills,
        &skills.join("no-made"),
        &skills.join("no-state"),
        WAIT,
        None,
        &b"{\"remove\": \"desk-tutor\"}\n"[..],
        &mut answer,
    )
    .unwrap();
    let waited = asked.elapsed();
    let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
    assert!(!ok);
    assert_eq!(answer["refused"], package::BUSY);
    assert_eq!(package::BUSY, "the lamp's skills can't be changed now");
    assert!(
        waited >= WAIT && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    held.unlock().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(left(&skills), ["desk-tutor"]);

    // Given back within the binary's own wait, the remove goes on.
    held.lock().unwrap();
    let folder = skills.canonicalize().unwrap();
    let (answer, ok, journal) =
        ask_helper_while(&skills, b"{\"remove\": \"desk-tutor\"}\n", |helper| {
            let started = Instant::now();
            while !has_open(helper.id(), &folder) {
                if let Some(status) = helper.try_wait().unwrap() {
                    panic!(
                        "the helper ended ({status}) before it was seen waiting for the held folder"
                    );
                }
                // A first start on a Mac takes up to 5 s.
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "the helper never opened the skills folder"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(300));
            held.unlock().unwrap();
        });
    assert!(ok, "{answer}: {journal}");
    assert!(left(&skills).is_empty());
}

/// Whether the process `pid` has `folder` open: `/proc` on Linux, `lsof`
/// on a Mac.
fn has_open(pid: u32, folder: &Path) -> bool {
    if let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) {
        return fds
            .flatten()
            .any(|fd| fs::read_link(fd.path()).is_ok_and(|to| to == folder));
    }
    std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-Fn"])
        .output()
        .is_ok_and(|listed| {
            String::from_utf8_lossy(&listed.stdout)
                .lines()
                .any(|line| line.strip_prefix('n').map(Path::new) == Some(folder))
        })
}

/// A skill renamed aside whose `.removed-NAME-PID` could not then be
/// deleted is gone for the running program, so it is answered done; where
/// it was set aside goes to the journal alone, never into the answer (the
/// reviewer's P3 2 on #1890).
#[test]
fn a_skill_renamed_aside_but_not_deleted_is_answered_done_its_path_in_the_journal_only() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let skill = tutor(&skills, "1.0.0");
    // A folder inside it whose file can't be deleted: the rename goes, the
    // delete after it does not.
    let stuck = skill.join("stuck");
    fs::create_dir(&stuck).unwrap();
    fs::write(stuck.join("kept"), "x").unwrap();
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o555)).unwrap();

    let (answer, ok, journal) = ask_helper_journal(&skills, b"{\"remove\": \"desk-tutor\"}\n");
    let names = left(&skills);
    for name in &names {
        let _ = fs::set_permissions(
            skills.join(name).join("stuck"),
            fs::Permissions::from_mode(0o755),
        );
    }
    assert!(ok, "{answer}");
    assert_eq!(
        answer["done"],
        "removed desk-tutor: a running lamp-skills lets it go within 2 s"
    );
    assert_eq!(answer.as_object().unwrap().len(), 1, "{answer}");
    let said = answer.to_string();
    assert!(
        !said.contains(".removed-") && !said.contains(&*skills.to_string_lossy()),
        "{said}"
    );
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with(".removed-desk-tutor-"), "{names:?}");
    assert!(journal.contains(&names[0]), "{journal}");
    assert!(journal.contains("was not deleted"), "{journal}");
    // By hand, the same is said as not finished, its path with it.
    let skill = tutor(&skills, "1.0.0");
    let stuck = skill.join("stuck");
    fs::create_dir(&stuck).unwrap();
    fs::write(stuck.join("kept"), "x").unwrap();
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o555)).unwrap();
    let why = package::remove(&skills, "desk-tutor").unwrap_err();
    for name in left(&skills) {
        let _ = fs::set_permissions(
            skills.join(name).join("stuck"),
            fs::Permissions::from_mode(0o755),
        );
    }
    assert!(why.starts_with("removed desk-tutor, but "), "{why}");
    assert!(why.contains(".removed-desk-tutor-"), "{why}");
}

/// The helper checks each request itself, whoever connects: a core skill
/// and a name that is not a skill's are refused with nothing touched, and
/// only `{"remove": NAME}` is taken (#1722, the first builder's 18:33).
#[test]
fn the_manage_helper_refuses_core_skills_and_bad_names_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    copy(&skills_root().join("dance"), &skills.join("dance"));
    tutor(&skills, "1.0.0");
    let victim = dir.path().join("victim");
    fs::create_dir_all(&victim).unwrap();

    let (answer, ok) = ask_helper(&skills, b"{\"remove\": \"dance\"}\n");
    assert!(!ok);
    assert_eq!(
        answer["refused"],
        "dance is a core skill: it comes with the lamp and can't be removed"
    );
    for line in [
        &b"{\"remove\": \"..\"}\n"[..],
        b"{\"remove\": \"../victim\"}\n",
        b"{\"remove\": \"desk-tutor/../dance\"}\n",
        b"{\"remove\": \"desk-tutor\\u0000\"}\n",
        b"{\"remove\": \".removed-desk-tutor-1\"}\n",
        b"{\"remove\": \"Desk-tutor\"}\n",
        b"{\"remove\": \"\"}\n",
    ] {
        let (answer, ok) = ask_helper(&skills, line);
        assert!(!ok, "{}", String::from_utf8_lossy(line));
        assert!(
            answer["refused"]
                .as_str()
                .unwrap()
                .ends_with("is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"),
            "{answer}"
        );
    }
    let long = format!("{{\"remove\": \"{}\"}}\n", "a".repeat(65));
    let (answer, ok) = ask_helper(&skills, long.as_bytes());
    assert!(
        !ok && answer["refused"]
            .as_str()
            .unwrap()
            .contains("is not a skill's name")
    );
    for line in [
        &b"{\"remove\": 7}\n"[..],
        b"{\"remove\": \"desk-tutor\", \"also\": 1}\n",
        b"[\"desk-tutor\"]\n",
        b"remove desk-tutor\n",
        b"",
    ] {
        let (answer, ok) = ask_helper(&skills, line);
        assert!(!ok);
        assert_eq!(answer["refused"], package::NOT_A_REQUEST);
    }
    // Past its bound, the request is cut and taken for none.
    let big = format!(
        "{{\"remove\": \"desk-tutor\", \"pad\": \"{}\"}}\n",
        "x".repeat(5_000)
    );
    let (answer, ok) = ask_helper(&skills, big.as_bytes());
    assert!(
        !ok && answer["refused"]
            .as_str()
            .unwrap()
            .starts_with("a request is")
    );
    // An install is a URL in an object, nothing else.
    let (answer, ok) = ask_helper(&skills, b"{\"install\": \"/tmp/x.tar\"}\n");
    assert!(!ok);
    assert_eq!(answer["refused"], package::NOT_A_REQUEST);
    assert!(victim.exists());
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);

    let (answer, ok) = ask_helper(&skills, b"{\"remove\": \"desk-tutor\"}\n");
    assert!(ok, "{answer}");
    assert!(
        answer["done"]
            .as_str()
            .unwrap()
            .starts_with("removed desk-tutor")
    );
    assert_eq!(left(&skills), ["dance"]);
}

/// #2815: the app's off and on, asked of the manage helper: a skill turned
/// off is in the state folder's `off`, where a running lamp-skills reads it
/// again and a started one reads it; on takes it out. A skill that is part
/// of the lamp itself is refused with why, and nothing is kept.
#[test]
fn the_apps_off_and_on_are_kept_and_a_skill_of_the_lamp_itself_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let state = dir.path().join("state");
    fs::create_dir_all(&state).unwrap();
    tutor(&skills, "1.0.0");
    copy(
        &skills_root().join("night-sleep"),
        &skills.join("night-sleep"),
    );
    let ask = |line: &str| {
        let mut answer = Vec::new();
        let ok = package::asked(
            &skills,
            &skills.join("no-made"),
            &state,
            package::ASKED_LOCK_WAIT,
            None,
            line.as_bytes(),
            &mut answer,
        )
        .unwrap();
        (
            serde_json::from_slice::<serde_json::Value>(&answer).unwrap(),
            ok,
        )
    };
    let (loaded, _) = lamp_skills::load_all(&skills);
    let mut running = lamp_skills::Engine::new(loaded, true);
    running.keep_off_in(&state).unwrap();

    let (answer, ok) = ask("{\"off\": \"desk-tutor\"}\n");
    assert!(ok, "{answer}");
    assert_eq!(answer["done"], "turned desk-tutor off");
    assert_eq!(
        fs::read_to_string(state.join("off")).unwrap(),
        "desk-tutor\n"
    );
    assert!(running.reread_off());
    assert!(running.stopped("desk-tutor", 0));
    let (loaded, _) = lamp_skills::load_all(&skills);
    let mut started = lamp_skills::Engine::new(loaded, true);
    assert_eq!(started.keep_off_in(&state).unwrap(), ["desk-tutor"]);

    let (answer, ok) = ask("{\"on\": \"desk-tutor\"}\n");
    assert!(ok, "{answer}");
    assert_eq!(answer["done"], "turned desk-tutor on");
    assert_eq!(fs::read_to_string(state.join("off")).unwrap(), "");
    assert!(running.reread_off());
    assert!(!running.stopped("desk-tutor", 0));

    let (answer, ok) = ask("{\"off\": \"night-sleep\"}\n");
    assert!(!ok);
    assert_eq!(
        answer["refused"],
        "night-sleep is part of how the lamp itself works, so it can't be turned off"
    );
    assert_eq!(fs::read_to_string(state.join("off")).unwrap(), "");
    let (answer, _) = ask("{\"off\": \"no-such\"}\n");
    assert_eq!(answer["refused"], "no skill no-such on this lamp");
    let (answer, _) = ask("{\"off\": \"desk-tutor\", \"on\": \"desk-tutor\"}\n");
    assert_eq!(answer["refused"], package::NOT_A_REQUEST);
}

/// #3504's piece C: the owner's yes to a skill's camera, asked of the
/// manage helper, is kept in the state folder's `camera`, a name a line,
/// and taken back. A running lamp-skills reads it at each call that would
/// give the picture: a skill that is not core sees it only while its yes is
/// kept, and a yes taken back holds from the next call, nothing restarted.
/// A core skill that asks the camera is the lamp's own and needs none. A
/// package that asks the camera, installed by hand, is not refused.
#[test]
fn the_owners_camera_yes_is_kept_read_at_each_call_and_taken_back() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let state = dir.path().join("state");
    fs::create_dir_all(&state).unwrap();
    copy(
        &skills_root().join("point-light"),
        &skills.join("point-light"),
    );
    // By hand, with no yes kept: put on all the same.
    let packed = package::pack(
        &tutor(&dir.path().join("src"), "1.0.0"),
        &dir.path().join("out"),
    )
    .unwrap();
    package::install(&skills, &packed).unwrap();
    let ask = |line: &str| {
        let mut answer = Vec::new();
        let ok = package::asked(
            &skills,
            &skills.join("no-made"),
            &state,
            package::ASKED_LOCK_WAIT,
            None,
            line.as_bytes(),
            &mut answer,
        )
        .unwrap();
        (
            serde_json::from_slice::<serde_json::Value>(&answer).unwrap(),
            ok,
        )
    };
    let camera = state.join(lamp_skills::CAMERA_FILE);
    let (loaded, _) = lamp_skills::load_all(&skills);
    let mut running = lamp_skills::Engine::new(loaded, true);
    running.keep_off_in(&state).unwrap();
    let not_yet = Some("the owner hasn't said yes to its camera");
    assert_eq!(running.picture_refused("desk-tutor").as_deref(), not_yet);
    assert_eq!(running.picture_refused("point-light"), None);

    let (answer, ok) = ask("{\"camera\": \"desk-tutor\"}\n");
    assert!(ok, "{answer}");
    assert_eq!(answer["done"], "desk-tutor may see the camera's picture");
    assert_eq!(fs::read_to_string(&camera).unwrap(), "desk-tutor\n");
    assert_eq!(running.picture_refused("desk-tutor"), None);
    // Given twice is kept once; a name not on the lamp yet is kept too, as
    // the Store installs a package that asks the camera only after it.
    assert!(ask("{\"camera\": \"desk-tutor\"}\n").1);
    assert!(ask("{\"camera\": \"from-the-store\"}\n").1);
    assert_eq!(
        fs::read_to_string(&camera).unwrap(),
        "desk-tutor\nfrom-the-store\n"
    );

    let (answer, ok) = ask("{\"camera_off\": \"desk-tutor\"}\n");
    assert!(ok, "{answer}");
    assert_eq!(
        answer["done"],
        "desk-tutor may not see the camera's picture"
    );
    assert_eq!(fs::read_to_string(&camera).unwrap(), "from-the-store\n");
    assert_eq!(running.picture_refused("desk-tutor").as_deref(), not_yet);
    assert_eq!(running.picture_refused("point-light"), None);

    let (answer, ok) = ask("{\"camera\": \"../off\"}\n");
    assert!(!ok);
    assert!(
        answer["refused"]
            .as_str()
            .unwrap()
            .contains("is not a skill's name"),
        "{answer}"
    );
    let (answer, _) = ask("{\"camera\": true}\n");
    assert_eq!(answer["refused"], package::NOT_A_REQUEST);
    assert_eq!(fs::read_to_string(&camera).unwrap(), "from-the-store\n");
    // With no state folder no yes is kept, so none is given.
    let (loaded, _) = lamp_skills::load_all(&skills);
    let bare = lamp_skills::Engine::new(loaded, true);
    assert_eq!(bare.picture_refused("desk-tutor").as_deref(), not_yet);
    assert_eq!(bare.picture_refused("point-light"), None);
}

/// The reviewer's P2 on #2878: root's helper writes `off` in a folder the
/// skills' user owns, so a link planted where the file beside goes is
/// refused and its target untouched, and a link at `off` is not read
/// through. A plain file left there by a cut-off run is replaced.
#[test]
fn the_off_file_is_never_written_or_read_through_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim");
    fs::write(&victim, "not yours\n").unwrap();
    let off = dir.path().join("off");
    std::os::unix::fs::symlink(&victim, off.with_extension("new")).unwrap();
    let names = ["desk-tutor".to_owned()].into();
    assert!(lamp_skills::keep_names(&off, &names).is_err());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "not yours\n");
    assert!(!off.exists());

    fs::remove_file(off.with_extension("new")).unwrap();
    fs::write(off.with_extension("new"), "left\n").unwrap();
    lamp_skills::keep_names(&off, &names).unwrap();
    assert_eq!(fs::read_to_string(&off).unwrap(), "desk-tutor\n");
    assert!(!off.with_extension("new").exists());

    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(&off, &linked).unwrap();
    assert!(lamp_skills::read_names(&linked).is_err());
}

/// The reviewer's P1 on #2878: the state folder stays 0700, as the daily
/// journal requires, and the app's off kept there leaves the journal
/// opening as before.
#[test]
fn the_daily_journal_opens_beside_the_apps_off_in_a_0700_state_folder() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let state = dir.path().join("state");
    fs::create_dir_all(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    tutor(&skills, "1.0.0");
    package::turn(
        &skills,
        &state,
        "desk-tutor",
        true,
        package::ASKED_LOCK_WAIT,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(state.join("off")).unwrap(),
        "desk-tutor\n"
    );
    lamp_skills::daily::Journal::open(state.join("daily.json")).unwrap();
    let units = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let skills_unit = fs::read_to_string(units.join("lamp-skills.service")).unwrap();
    assert!(skills_unit.contains("\nStateDirectoryMode=0700\n"));
    let helper = fs::read_to_string(units.join("lamp-skills-manage@.service")).unwrap();
    assert!(helper.contains("\nCapabilityBoundingSet=CAP_DAC_OVERRIDE\n"));
    assert!(
        helper.contains("\nReadWritePaths=/var/lib/lampOS/skills -/var/lib/lampos-skills-state\n")
    );
}
