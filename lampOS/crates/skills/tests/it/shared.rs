//! Shared packages (decision 4, "sharing from day one"; Ten on #3504, 28 Oct):
//! a package its author signs (`lamp-skills pack --key-file KEY --author`)
//! carries their public key, so any lamp checks it; the push path keeps a
//! skill's name with that author (`control.md`, "Skills pushed from a
//! computer"). A package unsigned, signed as the Store signs, changed after
//! signing, or whose signature names another key than it carries is
//! refused.
use lamp_skills::package::{self, MANIFEST, SIGNATURE, check_author, read_shared_package_bytes};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{
    fs,
    io::Read,
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

/// An author's key, from a seed: `which` tells two authors apart.
fn key(which: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[which; 32]).unwrap()
}

fn fingerprint(key: &Ed25519KeyPair) -> String {
    let public: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
    lamp_update::manifest::fingerprint(&public)
}

/// Desk tutor at `version`, packed and signed by `key`, as its author
/// (`author`) or as the Store signs (not): the package's bytes.
fn packed(dir: &Path, key: Option<&Ed25519KeyPair>, version: &str, author: bool) -> Vec<u8> {
    let at = dir.join(format!("src-{version}-{author}-{}", key.is_some()));
    let skill = at.join("desk-tutor");
    copy(&skills_root().join("desk-tutor"), &skill);
    let toml = fs::read_to_string(skill.join("skill.toml")).unwrap();
    fs::write(
        skill.join("skill.toml"),
        toml.replace("version = \"1.0.0\"", &format!("version = \"{version}\"")),
    )
    .unwrap();
    let path = package::pack_signed_by(&skill, &at.join("out"), key, author).unwrap();
    fs::read(path).unwrap()
}

/// A package's entries, in order: its name and bytes.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = tar::Archive::new(bytes);
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}

/// One entry's bytes in a package.
fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    entries(bytes)
        .into_iter()
        .find(|(path, _)| path == name)
        .unwrap()
        .1
}

/// The package again with one entry changed by `change`: someone's hand
/// between the author and the lamp.
fn changed(bytes: &[u8], name: &str, change: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in entries(bytes) {
        let data = if path == name { change(&data) } else { data };
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, &path, data.as_slice())
            .unwrap();
    }
    builder.into_inner().unwrap()
}

#[test]
fn an_authors_package_carries_their_key_and_names_them() {
    let dir = tempfile::tempdir().unwrap();
    let author = key(7);
    let bytes = packed(dir.path(), Some(&author), "1.0.0", true);
    let package = read_shared_package_bytes(&bytes).unwrap();
    assert_eq!(
        package.author.as_deref(),
        Some(fingerprint(&author).as_str())
    );
    assert_eq!(package.manifest.name, "desk-tutor");
    // The signature file carries the key itself, which the Store's doesn't.
    let checked = check_author(&entry(&bytes, MANIFEST), Some(&entry(&bytes, SIGNATURE))).unwrap();
    assert_eq!(checked.fingerprint, fingerprint(&author));
    assert_eq!(checked.public.as_slice(), author.public_key().as_ref());
    // Each author's package names its own key: two authors are told apart.
    let other =
        read_shared_package_bytes(&packed(dir.path(), Some(&key(9)), "1.0.1", true)).unwrap();
    assert_eq!(other.author.as_deref(), Some(fingerprint(&key(9)).as_str()));
    assert_ne!(other.author, package.author);
    // A Store-style package reads as before, its signature the Store's to check.
    let store = packed(dir.path(), Some(&author), "1.0.2", false);
    assert!(
        !String::from_utf8(entry(&store, SIGNATURE))
            .unwrap()
            .contains("public")
    );
}

#[test]
fn unsigned_store_signed_changed_or_misnamed_packages_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let author = key(7);
    let good = packed(dir.path(), Some(&author), "1.0.0", true);
    let refused = |bytes: &[u8], said: &str| {
        let why = read_shared_package_bytes(bytes).unwrap_err();
        assert!(why.contains(said), "{said}: {why}");
    };
    refused(
        &packed(dir.path(), None, "1.0.0", false),
        "a shared skill must be signed by its author",
    );
    refused(
        &packed(dir.path(), Some(&author), "1.0.0", false),
        "carries none: a shared skill carries its author's public key",
    );
    // The manifest changed after signing: a version raised by hand.
    refused(
        &changed(&good, MANIFEST, |data| {
            String::from_utf8(data.to_vec())
                .unwrap()
                .replace("\"1.0.0\"", "\"9.0.0\"")
                .into_bytes()
        }),
        "the package was changed after its author signed it",
    );
    // A file changed, the manifest left as signed.
    refused(
        &changed(&good, "desk-tutor/SKILL.md", |data| {
            let mut data = data.to_vec();
            data.extend_from_slice(b"\nAnd one more line.\n");
            data
        }),
        "its size or sha256 is not what lamp-skill.json says",
    );
    // A signature that names one key and carries another.
    refused(
        &changed(&good, SIGNATURE, |data| {
            String::from_utf8(data.to_vec())
                .unwrap()
                .replace(&fingerprint(&author), &fingerprint(&key(9)))
                .into_bytes()
        }),
        &format!(
            "names key {}, and the key it carries is",
            fingerprint(&key(9))
        ),
    );
}
