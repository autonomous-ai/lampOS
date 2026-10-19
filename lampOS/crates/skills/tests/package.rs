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
        said.contains("it carries no signature, and none is checked yet"),
        "{said}"
    );
    assert!(
        said.contains("It may use: speak, light, body, look."),
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
        said.contains("its signature not checked: signing is not built yet"),
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
            "speak, light, body, look"
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
            "reads humans/faces/tracked, voice/state; speak, light, body"
        ]
    );
}

/// The manage helper as systemd runs it for a connection to its socket
/// (`lamp-skills-manage.socket`, `Accept=yes`): the real program, `lamp-skills
/// asked`, its standard input and output one end of a Unix socket, asked
/// `line` straight, with nobody before it to check the request (#1722).
fn ask_helper(skills: &Path, line: &[u8]) -> (serde_json::Value, bool) {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    let (mut ours, theirs) = UnixStream::pair().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
        .args(["asked", "--dir"])
        .arg(skills)
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(
            theirs.try_clone().unwrap(),
        )))
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ours.write_all(line).unwrap();
    ours.shutdown(std::net::Shutdown::Write).unwrap();
    let mut answer = String::new();
    std::io::BufReader::new(&ours)
        .read_line(&mut answer)
        .unwrap();
    let status = child.wait().unwrap();
    (serde_json::from_str(&answer).unwrap(), status.success())
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
        assert_eq!(
            answer["refused"],
            "a request is one line, {\"remove\": NAME}"
        );
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
    let (answer, ok) = ask_helper(&skills, b"{\"install\": \"/tmp/x.tar\"}\n");
    assert!(
        !ok && answer["refused"]
            .as_str()
            .unwrap()
            .starts_with("install is not taken here yet")
    );
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
