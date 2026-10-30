//! Core and installable skills (`docs/skills.md`, "Core and installable
//! skills"). A core skill comes with the lamp, as a phone's own apps do:
//! `lamp-update install` writes it and nothing here removes or replaces it.
//! An installable one is its owner's to put on and take off a running lamp,
//! from a folder or from a package, the `.tar` the Skills Store will carry.
//! Nothing else is restarted: the running `lamp-skills` looks at its folder
//! every two seconds and loads what changed.
//!
//! A package is a tar of one skill's folder and a manifest, `lamp-skill.json`,
//! naming the format, the skill, its version and every file with its size
//! and SHA-256, each checked before anything is installed, and a signature,
//! `lamp-skill.sig`: Ed25519 over `lamp-skill.json`'s exact bytes. A package
//! installed by hand from a file on the lamp is not held to its signature;
//! one from the Skills Store, fetched from a URL by root's manage helper
//! ([`install_from_url`], `docs/skills.md`, "From the Skills Store"), is
//! installed only when it is signed with the Store's key ([`store_keys`]:
//! the build's [`BUILD_STORE_KEY`], else the factory's [`STORE_KEY`]),
//! checked before anything is written.
use super::{Skill, SkillKind, load, load_each, plain_name, version_ok};
use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_update::fetch::Channel;
use ring::signature::{ED25519, Ed25519KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    io::{BufRead, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The package format this program writes and reads.
pub const FORMAT: u32 = 1;
/// The manifest's name at the top of a package.
pub const MANIFEST: &str = "lamp-skill.json";
/// The Store's signature over `lamp-skill.json`'s exact bytes, one line of
/// JSON, `{"key": FINGERPRINT, "signature": BASE64}` ([`signature_for`]): the
/// key's fingerprint as `lamp-update` gives it (16 hex), so a package signed
/// with another key is told apart from a changed one, and the 64 bytes of
/// the Ed25519 signature. The manifest names every file with its SHA-256,
/// so the signature covers every byte of the skill.
pub const SIGNATURE: &str = "lamp-skill.sig";
/// The Skills Store's own public key, as the factory provisions it (step
/// 3's `--store-key`). Provisioned separately from the update channel's
/// key; an update key is never a fallback for this one. A build's key
/// ([`BUILD_STORE_KEY`]) wins over it.
pub const STORE_KEY: &str = "/etc/lampOS/skills/store-key.pub";
/// The Store's key as a build ships it (#3692), in the skills folder
/// itself (`/var/lib/lampOS/skills/store-key.pub` on a product lamp): the
/// one place under the five a fixed settings writer from build 7071 on
/// takes (`lamp_update::manifest::SETTING_FOLDERS`) that the Store's
/// readers can see. Root's, 0644, a plain file the loader passes over
/// ([`super::load_each`] loads folders only), with a name no skill can
/// have (a skill's has no dot), so no remove or install reaches it. One or
/// two keys, one a line (a rotation: the Store's new and old), as
/// `lamp_update::manifest::read_public_keys` reads them.
pub const BUILD_STORE_KEY: &str = "store-key.pub";
/// The Store's switch, `on` or off: `lamp-cloud` answers `skills.install`
/// only with it, and the manage helper itself takes an `install` only with
/// it in its own environment (`lamp-skills-manage@.service`'s drop-in), so
/// a process that reaches its socket past `lamp-cloud` installs nothing
/// while the Store is off. A remove needs no switch of its own.
pub const STORE_SWITCH: &str = "LAMP_SKILLS_STORE";
/// What an install is told while the Store's switch is off, by
/// `lamp-cloud` and by the manage helper alike.
pub const STORE_OFF: &str = "not on this lamp: skills.install";

/// Whether the Store's switch ([`STORE_SWITCH`]) is on: `on` only.
pub fn store_switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}
/// The Store is usable only when explicitly on and its own keys are
/// readable and valid ([`store_keys`]). Check before constructing an HTTP
/// client; removal never needs a Store key. The install path reads the keys
/// again before every fetch.
pub fn store_ready(value: Option<&str>, skills: &Path, key_file: &Path) -> bool {
    store_switched_on(value) && store_keys(skills, key_file).is_ok()
}
/// The longest a package's URL may be.
pub const MOST_URL: usize = 2_048;
/// The longest a package is fetched for, redirects among it. A Store
/// package is at most [`MOST_PACKAGE_BYTES`], 18 MB: about 1 MB/s at least.
/// `lamp-cloud` waits a little longer for the helper's answer.
pub const FETCH_WITHIN: Duration = Duration::from_secs(25);
/// The most bytes of a signature file: its JSON is under 150.
const MOST_SIGNATURE_BYTES: usize = 1_024;
/// The most files a skill may have, its tests among them.
pub const MOST_FILES: usize = 512;
/// The most bytes a skill's files may come to (the largest of the lamp's
/// own, Stuffy air with its recordings, is about 1.4 MB).
pub const MOST_BYTES: u64 = 16 * 1024 * 1024;
/// A package's own size at most: its files and the tar's headers.
pub const MOST_PACKAGE_BYTES: u64 = MOST_BYTES + 2 * 1024 * 1024;
/// The most parts a file's path in a skill may have (`tests/CASE/FILE`).
const MOST_DEPTH: usize = 4;
/// The most functions a session is given, the voice's own and the actions
/// part's told first, then the skills' in their folders' order
/// (`docs/tools.md`): past it, the last are left out.
pub const SESSION_FUNCTIONS: usize = 32;

/// `lamp-skill.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    pub version: String,
    pub files: Vec<Entry>,
}

/// One file of a skill, as its manifest lists it: its path in the skill's
/// folder, `/` between its parts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// A skill's files by their path in its folder, `/` between the parts.
pub type Files = BTreeMap<String, Vec<u8>>;

/// A package read and checked: its manifest, the skill's files, each as the
/// manifest says, the signature it carries, if any, the fingerprint of the
/// key it was checked against, when it was, and the SHA-256 of the whole
/// package, the `.tar`'s own bytes, as the Store's catalogue names it.
#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub files: Files,
    pub signature: Option<Vec<u8>>,
    pub signed_by: Option<String>,
    /// The author's key, for a shared package read with
    /// [`read_shared_package_bytes`]: the push path pins a skill's name to
    /// all 32 bytes of it, not the fingerprint, which is only for showing
    /// (Eighth's P3 on #3931; `control.md`, "Skills pushed from a computer").
    pub author: Option<Author>,
    pub sha256: String,
}

/// What a skill put on from a package keeps of it, in its own folder: the
/// package's SHA-256 and its `lamp-skill.json` as installed, so the app's
/// list can say which package the skill is ([`installed_from`]). Hidden, so
/// the loader, `pack` and an install from a folder pass over it, and a
/// package can't carry one of its own (no path in a package begins with a
/// dot): only an install from a package writes it, into the folder before
/// the folder is moved in, so it comes and goes with the skill, replaced
/// whole by the next install and deleted by a remove. A skill that comes
/// with the lamp, or put on from a folder, has none.
pub const INSTALLED_FROM: &str = ".package.json";

/// [`INSTALLED_FROM`]'s contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installed {
    /// The SHA-256 of the package's bytes, lowercase hex.
    pub sha256: String,
    /// The package's `lamp-skill.json`.
    pub manifest: Manifest,
}

/// The most bytes of [`INSTALLED_FROM`] read: a manifest of [`MOST_FILES`]
/// files is under 100 KB.
const MOST_INSTALLED_BYTES: u64 = 256 * 1024;

/// The package the installable skill in `folder` was put on from, when it
/// was and the folder is still as that install left it: the record names
/// the folder's skill, its SHA-256 is one, and its version is the one
/// `skill.toml` says now (a version changed by hand is no longer that
/// package's). Otherwise none: a core skill, one put on from a folder, or
/// one installed before skills kept this.
pub fn installed_from(folder: &Path) -> Option<Installed> {
    let mut bytes = Vec::new();
    fs::File::open(folder.join(INSTALLED_FROM))
        .ok()?
        .take(MOST_INSTALLED_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let installed: Installed = serde_json::from_slice(&bytes).ok()?;
    (kind_of(folder) != SkillKind::Core
        && sha256_ok(&installed.sha256)
        && folder.file_name().and_then(|name| name.to_str())
            == Some(installed.manifest.name.as_str())
        && version_of(folder).as_deref() == Some(installed.manifest.version.as_str()))
    .then_some(installed)
}

/// `lamp-skill.sig`'s one line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    key: String,
    /// An author's public key, base64 of its 32 bytes, in a shared
    /// package ([`check_author`]): a lamp knows the Store's key, never an
    /// author's. Absent in a Store package, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    public: Option<String>,
    signature: String,
}

/// The author a shared package is signed by: their key's fingerprint, as
/// `lamp-update` gives it (16 hex), and the key itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub fingerprint: String,
    pub public: [u8; 32],
}

/// What `lamp-skill.sig` holds for these manifest bytes, signed with `key`:
/// what the Store's side writes into a package (`lamp-skills pack
/// --key-file`).
pub fn signature_for(manifest: &[u8], key: &Ed25519KeyPair) -> Result<Vec<u8>, String> {
    signed_by(manifest, key, false)
}

/// What `lamp-skill.sig` holds for a package its author shares: as
/// [`signature_for`], with the author's public key, so any lamp can check
/// it (`lamp-skills pack --key-file --author`).
pub fn author_signature_for(manifest: &[u8], key: &Ed25519KeyPair) -> Result<Vec<u8>, String> {
    signed_by(manifest, key, true)
}

fn signed_by(manifest: &[u8], key: &Ed25519KeyPair, author: bool) -> Result<Vec<u8>, String> {
    use ring::signature::KeyPair;
    let public: [u8; 32] = key
        .public_key()
        .as_ref()
        .try_into()
        .map_err(|_| "the key's public half is not 32 bytes".to_owned())?;
    let signed = Signed {
        key: lamp_update::manifest::fingerprint(&public),
        public: author.then(|| STANDARD.encode(public)),
        signature: STANDARD.encode(key.sign(manifest).as_ref()),
    };
    let mut bytes = serde_json::to_vec(&signed).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Whether a shared package's `signature` is its author's over the
/// `manifest` bytes: the key it carries, the fingerprint it names being
/// that key's, and the signature that key's. The author, or why not.
/// Asked before the manifest is read, as [`check_signature`] is; the
/// manifest's hashes then cover every file.
pub fn check_author(manifest: &[u8], signature: Option<&[u8]>) -> Result<Author, String> {
    let signature = signature.ok_or(format!(
        "it carries no {SIGNATURE}: a shared skill must be signed by its author \
         (lamp-skills pack --key-file KEY --author)"
    ))?;
    if signature.len() > MOST_SIGNATURE_BYTES {
        return Err(format!("{SIGNATURE} is too large to be a signature"));
    }
    let signed: Signed = serde_json::from_slice(signature).map_err(|_| {
        format!("{SIGNATURE} is not a signature: {{\"key\", \"public\", \"signature\"}}")
    })?;
    let public = signed.public.ok_or(format!(
        "{SIGNATURE} names a key but carries none: a shared skill carries its author's public key"
    ))?;
    let public: [u8; 32] = STANDARD
        .decode(&public)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(format!(
            "{SIGNATURE}: the author's key is not 32 bytes of base64"
        ))?;
    let fingerprint = lamp_update::manifest::fingerprint(&public);
    if fingerprint != signed.key {
        return Err(format!(
            "{SIGNATURE} names key {}, and the key it carries is {fingerprint}",
            if signed.key.len() == 16 && signed.key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                signed.key.as_str()
            } else {
                "that is not a key's fingerprint"
            }
        ));
    }
    let raw = STANDARD
        .decode(&signed.signature)
        .map_err(|_| format!("{SIGNATURE}: the signature is not base64"))?;
    UnparsedPublicKey::new(&ED25519, &public)
        .verify(manifest, &raw)
        .map_err(|_| {
            format!(
                "{SIGNATURE} does not match {MANIFEST}: the package was changed after its author signed it"
            )
        })?;
    Ok(Author {
        fingerprint,
        public,
    })
}

/// Whether `signature`, a package's `lamp-skill.sig`, is one of `keys`'
/// over the `manifest` bytes, the key its fingerprint names (two during a
/// rotation): that key's fingerprint, or why not. Asked before the
/// manifest is even read, so nothing a package says is taken before its
/// signature is known to be the channel's.
pub fn check_signature(
    manifest: &[u8],
    signature: Option<&[u8]>,
    keys: &[[u8; 32]],
) -> Result<String, String> {
    let ours = keys
        .iter()
        .map(lamp_update::manifest::fingerprint)
        .collect::<Vec<_>>()
        .join(" or ");
    let signature = signature.ok_or(format!(
        "it carries no {SIGNATURE}: a package from the Skills Store must be signed with the Store's \
         key ({ours})"
    ))?;
    if signature.len() > MOST_SIGNATURE_BYTES {
        return Err(format!("{SIGNATURE} is too large to be a signature"));
    }
    let signed: Signed = serde_json::from_slice(signature)
        .map_err(|_| format!("{SIGNATURE} is not a signature: {{\"key\", \"signature\"}}"))?;
    let Some(key) = keys
        .iter()
        .find(|key| lamp_update::manifest::fingerprint(key) == signed.key)
    else {
        // Said back only when it is a fingerprint's shape: nothing else a
        // package carries is echoed.
        let theirs =
            if signed.key.len() == 16 && signed.key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                signed.key.as_str()
            } else {
                "that is not a key's fingerprint"
            };
        return Err(format!(
            "signed with key {theirs}, and this lamp takes skills signed with the Store's key {ours}"
        ));
    };
    let raw = STANDARD
        .decode(&signed.signature)
        .map_err(|_| format!("{SIGNATURE}: the signature is not base64"))?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(manifest, &raw)
        .map_err(|_| {
            format!(
                "{SIGNATURE} does not match {MANIFEST}: the package was changed after it was signed"
            )
        })?;
    Ok(signed.key)
}

/// Lowercase hex of `bytes`' SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A skill's name: 1 to 64 lowercase letters, digits and hyphens.
pub fn skill_name_ok(name: &str) -> bool {
    plain_name(name) && !name.contains('_')
}

/// One part of a path in a skill: letters, digits, `-`, `_` and `.`, not
/// beginning with a dot (hidden, as a Mac's `._NAME`), at most 100 bytes.
fn part_ok(part: &str) -> bool {
    (1..=100).contains(&part.len())
        && !part.starts_with('.')
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// A file's path in a skill: 1 to `MOST_DEPTH` good parts.
fn path_ok(path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MOST_DEPTH || !parts.iter().all(|part| part_ok(part)) {
        return Err(format!(
            "{path:?}: a skill's paths are at most {MOST_DEPTH} parts of letters, digits, -, _ and ., \
             none beginning with a dot"
        ));
    }
    Ok(())
}

/// The plain file at `path`, opened without following a link at its end and
/// checked as opened: a regular file, and the one its folder listed
/// (`listed`, by its device and inode). A file swapped for a link or for
/// another file between the listing and the open is refused, never read:
/// run as root on a folder another user can write, a link swapped in would
/// otherwise copy a file only root may read into the skill, made 0644.
/// Opened without blocking too, so a fifo swapped in does not hang it.
fn open_plain(path: &Path, listed: &fs::Metadata) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                "a link: a skill is plain files and folders".to_owned()
            } else {
                error.to_string()
            }
        })?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if !opened.is_file() {
        return Err("not a plain file".into());
    }
    if (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return Err("changed while it was read: not the file its folder listed".into());
    }
    Ok(file)
}

/// Every file of the skill folder `dir` but hidden ones (their name begins
/// with a dot, as the loader passes them over). A link, or anything but a
/// plain file or folder, refuses it: a skill is plain files. Each file is
/// read as [`open_plain`] opens it.
pub fn read_folder(dir: &Path) -> Result<Files, String> {
    let mut files = Files::new();
    let mut total = 0u64;
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries =
            fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", folder.display()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(format!(
                    "{}: a name that is not UTF-8",
                    entry.path().display()
                ));
            };
            if name.starts_with('.') {
                continue;
            }
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let meta =
                fs::symlink_metadata(entry.path()).map_err(|error| format!("{path}: {error}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "{path}: a link: a skill is plain files and folders"
                ));
            }
            if meta.is_dir() {
                path_ok(&path)?;
                folders.push((entry.path(), path));
                continue;
            }
            if !meta.is_file() {
                return Err(format!("{path}: not a plain file"));
            }
            path_ok(&path)?;
            let too_big = || {
                format!("more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for a skill")
            };
            if total + meta.len() > MOST_BYTES || files.len() >= MOST_FILES {
                return Err(too_big());
            }
            let mut bytes = Vec::new();
            open_plain(&entry.path(), &meta)
                .and_then(|file| {
                    file.take(MOST_BYTES - total + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|error| error.to_string())
                })
                .map_err(|why| format!("{path}: {why}"))?;
            // What was read, which may be more than was listed.
            total += bytes.len() as u64;
            if total > MOST_BYTES {
                return Err(too_big());
            }
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

/// One plain file in a package, written the same way every time (no time,
/// no owner), so the same skill packs to the same bytes.
fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    builder
        .append_data(&mut header, path, bytes)
        .map_err(|error| format!("{path}: {error}"))
}

/// Packs the installable skill folder `dir` as `NAME-VERSION.tar` in `out`,
/// for the Store: the skill must load, say its version, and be installable
/// (a core skill comes with the lamp, never as a package). The package is
/// read back and checked as an install checks it.
pub fn pack(dir: &Path, out: &Path) -> Result<PathBuf, String> {
    pack_signed(dir, out, None)
}

/// [`pack`], and with `key`, the manifest signed ([`signature_for`]) into
/// `lamp-skill.sig`: a package the Skills Store can carry, when `key` is the
/// Store's private key. It is read back and its signature checked.
pub fn pack_signed(
    dir: &Path,
    out: &Path,
    key: Option<&Ed25519KeyPair>,
) -> Result<PathBuf, String> {
    pack_signed_by(dir, out, key, false)
}

/// As [`pack_signed`]; with `author`, the signature is an author's
/// ([`author_signature_for`]), carrying their public key, for a package
/// they share.
pub fn pack_signed_by(
    dir: &Path,
    out: &Path,
    key: Option<&Ed25519KeyPair>,
    author: bool,
) -> Result<PathBuf, String> {
    let skill = load(dir).map_err(|why| format!("not packed: {why}"))?;
    if skill.kind == SkillKind::Core {
        return Err(format!(
            "not packed: {} is a core skill: it comes with the lamp (lamp-update install), never as a package",
            skill.name
        ));
    }
    let Some(version) = skill.version.clone() else {
        return Err(format!(
            "not packed: {}'s skill.toml says no version: a package carries one, as version = \"1.0.0\"",
            skill.name
        ));
    };
    let files = read_folder(dir)?;
    let manifest = Manifest {
        format: FORMAT,
        name: skill.name.clone(),
        version: version.clone(),
        files: files
            .iter()
            .map(|(path, bytes)| Entry {
                path: path.clone(),
                size: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
    };
    let mut json = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    json.push(b'\n');
    let mut builder = tar::Builder::new(Vec::new());
    append(&mut builder, MANIFEST, &json)?;
    if let Some(key) = key {
        let signature = if author {
            author_signature_for(&json, key)?
        } else {
            signature_for(&json, key)?
        };
        append(&mut builder, SIGNATURE, &signature)?;
    }
    for (path, bytes) in &files {
        append(&mut builder, &format!("{}/{path}", skill.name), bytes)?;
    }
    let bytes = builder.into_inner().map_err(|error| error.to_string())?;
    fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let path = out.join(format!("{}-{version}.tar", skill.name));
    // Whole or not at all.
    let part = out.join(format!(".{}-{version}.tar.part", skill.name));
    fs::write(&part, &bytes)
        .and_then(|()| fs::rename(&part, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let read = read_package(&path)
        .map_err(|why| format!("{}: packed, then not read back: {why}", path.display()))?;
    if let Some(key) = key {
        use ring::signature::KeyPair;
        let public: [u8; 32] = key
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| "the key's public half is not 32 bytes".to_owned())?;
        check_signature(&json, read.signature.as_deref(), &[public])
            .map_err(|why| format!("{}: packed, then its signature: {why}", path.display()))?;
    }
    Ok(path)
}

/// The package at `path`, read and checked: a plain tar of a manifest, an
/// optional signature (kept, not checked: a file installed by hand) and one
/// skill's folder of plain files, every one of them in the manifest with its
/// size and SHA-256, and no other.
pub fn read_package(path: &Path) -> Result<Package, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    if meta.len() > MOST_PACKAGE_BYTES {
        return Err(format!(
            "more than {MOST_PACKAGE_BYTES} bytes: too big for a skill"
        ));
    }
    let mut bytes = Vec::new();
    open_plain(path, &meta).and_then(|file| {
        file.take(MOST_PACKAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())
    })?;
    read_package_bytes(&bytes, None)
}

/// A package from its bytes, read in memory and checked as [`read_package`]
/// checks it, with nothing written anywhere. With `keys`, its signature must
/// be one of those keys' over the manifest's exact bytes ([`check_signature`]),
/// checked once the tar is read and before the manifest is parsed or any
/// file is compared with it.
pub fn read_package_bytes(bytes: &[u8], keys: Option<&[[u8; 32]]>) -> Result<Package, String> {
    read_checked(bytes, keys.map_or(Check::None, Check::Store))
}

/// A shared package from its bytes (decision 4, "sharing from day one"):
/// read and checked as [`read_package_bytes`] does, and signed by its
/// author ([`check_author`]), checked before the manifest is parsed. Its
/// `author` is that key, which the push path keeps with the skill, so an
/// update must come from the same author. A key that is one of the
/// Store's (`store`, as [`store_keys`] reads them) is refused: anyone can
/// add the Store's public key to a Store package's signature, and it is
/// then no author's (Fourteen's P3 on #3931).
pub fn read_shared_package_bytes(bytes: &[u8], store: &[[u8; 32]]) -> Result<Package, String> {
    read_checked(bytes, Check::Author(store))
}

/// What a package's signature is held to as it is read.
enum Check<'a> {
    /// Kept, not checked: a file installed by hand.
    None,
    /// One of the Store's keys ([`check_signature`]).
    Store(&'a [[u8; 32]]),
    /// Its author's, the key it carries ([`check_author`]), never one of
    /// the Store's keys.
    Author(&'a [[u8; 32]]),
}

fn read_checked(bytes: &[u8], check: Check) -> Result<Package, String> {
    if bytes.len() as u64 > MOST_PACKAGE_BYTES {
        return Err(format!(
            "more than {MOST_PACKAGE_BYTES} bytes: too big for a skill"
        ));
    }
    let mut archive = tar::Archive::new(bytes);
    let (mut manifest, mut signature) = (None, None);
    let mut files = Files::new();
    let mut folder: Option<String> = None;
    let mut total = 0u64;
    let entries = archive
        .entries()
        .map_err(|error| format!("not a tar: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("not a tar: {error}"))?;
        let name = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|_| "a path that is not UTF-8".to_owned())?;
        let kind = entry.header().entry_type();
        let path = name.trim_end_matches('/').to_owned();
        // The skill's folder, and only it, before the first `/`; its path
        // in the folder after it. A file with no `/` is beside the folder.
        let mut within = |path: &str, folder_entry: bool| -> Result<Option<String>, String> {
            let (top, rest) = match path.split_once('/') {
                Some((top, rest)) => (top, Some(rest)),
                None if folder_entry => (path, None),
                None => return Ok(None),
            };
            if folder.as_deref().is_some_and(|folder| folder != top) {
                return Err(format!(
                    "{path}: a second folder beside {}: a package holds one skill",
                    folder.as_deref().unwrap_or_default()
                ));
            }
            folder = Some(top.to_owned());
            match rest {
                Some(rest) => path_ok(rest).map(|()| Some(rest.to_owned())),
                None => Ok(None),
            }
        };
        if kind.is_dir() {
            within(&path, true)?;
            continue;
        }
        if kind != tar::EntryType::Regular {
            return Err(format!(
                "{path}: a link, a device or another entry that is not a plain file: a package holds plain files"
            ));
        }
        let size = entry.size();
        total += size;
        if total > MOST_BYTES || files.len() >= MOST_FILES {
            return Err(format!(
                "more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for a skill"
            ));
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(size)
            .read_to_end(&mut data)
            .map_err(|error| format!("{path}: {error}"))?;
        if data.len() as u64 != size {
            return Err(format!("{path}: cut short"));
        }
        let kept = match path.as_str() {
            MANIFEST => manifest.replace(data).is_some(),
            SIGNATURE => signature.replace(data).is_some(),
            _ => match within(&path, false)? {
                Some(rest) => files.insert(rest, data).is_some(),
                None => {
                    return Err(format!(
                        "{path}: a file beside the skill's folder that is neither {MANIFEST} nor {SIGNATURE}"
                    ));
                }
            },
        };
        if kept {
            return Err(format!("{path}: twice in the package"));
        }
    }
    let manifest = manifest.ok_or(format!("no {MANIFEST}: not a skill's package"))?;
    let (signed_by, author) = match check {
        Check::None => (None, None),
        Check::Store(keys) => (
            Some(check_signature(&manifest, signature.as_deref(), keys)?),
            None,
        ),
        Check::Author(store) => {
            let author = check_author(&manifest, signature.as_deref())?;
            if store.contains(&author.public) {
                return Err(format!(
                    "{SIGNATURE} carries the Store's key {}: a Store package is \
                     installed from the Store, not shared",
                    author.fingerprint
                ));
            }
            (Some(author.fingerprint.clone()), Some(author))
        }
    };
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|error| format!("{MANIFEST}: {error}"))?;
    if manifest.format != FORMAT {
        return Err(format!(
            "{MANIFEST}: format {} is not one this lamp reads ({FORMAT})",
            manifest.format
        ));
    }
    if !skill_name_ok(&manifest.name) {
        return Err(format!(
            "{MANIFEST}: {:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens",
            manifest.name
        ));
    }
    if folder.as_deref() != Some(manifest.name.as_str()) {
        return Err(format!(
            "{MANIFEST} names {:?}, and the package's folder is {:?}",
            manifest.name,
            folder.unwrap_or_default()
        ));
    }
    if !version_ok(&manifest.version) {
        return Err(format!(
            "{MANIFEST}: version {:?} is not MAJOR.MINOR.PATCH",
            manifest.version
        ));
    }
    let mut listed = BTreeSet::new();
    for entry in &manifest.files {
        if !listed.insert(entry.path.as_str()) {
            return Err(format!("{MANIFEST} lists {} twice", entry.path));
        }
        let Some(data) = files.get(&entry.path) else {
            return Err(format!(
                "{} is in {MANIFEST} and not in the package",
                entry.path
            ));
        };
        if data.len() as u64 != entry.size || sha256_hex(data) != entry.sha256 {
            return Err(format!(
                "{}: its size or sha256 is not what {MANIFEST} says: the package was changed or damaged",
                entry.path
            ));
        }
    }
    if let Some(extra) = files.keys().find(|path| !listed.contains(path.as_str())) {
        return Err(format!("{extra} is in the package and not in {MANIFEST}"));
    }
    Ok(Package {
        manifest,
        files,
        signature,
        signed_by,
        author,
        sha256: sha256_hex(bytes),
    })
}

/// The kind a skill folder's `skill.toml` says, read on its own, so that a
/// folder that no longer loads is still known for what it is: installable
/// when it says none or cannot be read.
fn kind_of(dir: &Path) -> SkillKind {
    toml_of(dir)
        .and_then(|toml| SkillKind::read(toml.get("kind").and_then(toml::Value::as_str)).ok())
        .unwrap_or_default()
}

fn version_of(dir: &Path) -> Option<String> {
    toml_of(dir)?
        .get("version")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

/// A `MAJOR.MINOR.PATCH` version ([`version_ok`]) as three numbers to
/// order by, or none.
fn version_order(version: &str) -> Option<(u64, u64, u64)> {
    if !version_ok(version) {
        return None;
    }
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Why a Store package at `version` may not replace the skill `name`
/// installed at `installed`, or none when it may: it is the same version
/// or a newer one. A version that can't be ordered (none, or not
/// `MAJOR.MINOR.PATCH`) is refused, since the package may be the older:
/// the person removes the installed one first, and then installs.
fn older_refused(name: &str, installed: Option<&str>, version: &str) -> Option<String> {
    let said = installed.unwrap_or("with no version");
    match (version_order(version), installed.and_then(version_order)) {
        (Some(new), Some(old)) if new >= old => None,
        (Some(_), Some(_)) => Some(format!(
            "{name} {said} is on this lamp, and this package is the older {version}: \
             a skill is never put back to an older version"
        )),
        _ => Some(format!(
            "{name} {said} is on this lamp, and its version can't be compared with {version}: \
             remove it first, then install this one"
        )),
    }
}

fn toml_of(dir: &Path) -> Option<toml::Table> {
    fs::read_to_string(dir.join("skill.toml"))
        .ok()?
        .parse::<toml::Table>()
        .ok()
}

/// The folder and its files open to every user (the parts run as `lampos`)
/// and written only by their owner, whatever the umask of who installed it.
fn open_to_parts(dir: &Path) -> io::Result<()> {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            open_to_parts(&entry.path())?;
        } else {
            fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o644))?;
        }
    }
    Ok(())
}

/// A folder of `skills` the running program passes over (its name begins
/// with a dot) for this install's own use.
fn hidden_place(skills: &Path, what: &str, name: &str) -> PathBuf {
    skills.join(format!(".{what}-{name}-{}", std::process::id()))
}

/// The skills folder held by this install or remove for its whole run (an
/// advisory lock on the folder itself, given back when the run ends, killed
/// or not), so two never interleave: a staged or set-aside folder another
/// run finds is a killed run's, never one under way, and what one checks
/// of the folder is still so when it moves a skill in.
fn locked(skills: &Path) -> Result<fs::File, String> {
    let folder =
        fs::File::open(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    folder
        .lock()
        .map_err(|error| format!("{}: not locked: {error}", skills.display()))?;
    Ok(folder)
}

/// [`locked`], but waiting at most `wait` for another run to give the
/// folder back: then [`BUSY`]. The manage helper waits this way, so it never
/// goes on with a remove or an install after its asker has stopped waiting
/// for it.
fn locked_within(skills: &Path, wait: Duration) -> Result<fs::File, String> {
    let folder =
        fs::File::open(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    let until = Instant::now() + wait;
    loop {
        match folder.try_lock() {
            Ok(()) => return Ok(folder),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(fs::TryLockError::WouldBlock) => {
                eprintln!(
                    "lamp-skills: {}: held by another run for {} ms: nothing done",
                    skills.display(),
                    wait.as_millis()
                );
                return Err(BUSY.to_owned());
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(format!("{}: not locked: {error}", skills.display()));
            }
        }
    }
}

/// Why the manage helper did nothing when another run held the skills
/// folder past [`ASKED_LOCK_WAIT`]: the words `lamp-cloud` already gives the
/// app when the helper does not answer.
pub const BUSY: &str = "the lamp's skills can't be changed now";

/// The longest the manage helper waits for the skills folder's lock, under
/// `lamp-cloud`'s 5 s wait for a remove's answer, so a remove is done, or
/// refused, while `lamp-cloud` still waits for it, never after. An install
/// waits as long once its package is fetched and checked: with
/// [`FETCH_WITHIN`], under `lamp-cloud`'s 40 s wait for an install.
pub const ASKED_LOCK_WAIT: Duration = Duration::from_secs(4);

/// Whether `name` is what [`hidden_place`] names: `.install-NAME-PID` or
/// `.removed-NAME-PID`, a skill's name and a number, and nothing else.
fn left_by_a_run(name: &str) -> bool {
    [".install-", ".removed-"].iter().any(|what| {
        name.strip_prefix(what)
            .and_then(|rest| rest.rsplit_once('-'))
            .is_some_and(|(skill, pid)| {
                skill_name_ok(skill)
                    && (1..=10).contains(&pid.len())
                    && pid.bytes().all(|byte| byte.is_ascii_digit())
            })
    })
}

/// Deletes what a killed install or remove left in `skills` (called with
/// the folder [`locked`], so nothing under way), and names it: a folder
/// directly in `skills` whose name [`left_by_a_run`]. A link, a file, or a
/// folder of any other name is left as it is.
fn swept(skills: &Path) -> Vec<String> {
    let mut swept: Vec<String> = fs::read_dir(skills)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| left_by_a_run(name))
        .filter(|name| {
            let place = skills.join(name);
            fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_dir())
                && fs::remove_dir_all(&place).is_ok()
        })
        .collect();
    swept.sort();
    swept
}

/// The skill in `skills`, other than `name`, whose tool is `tool`, and its
/// kind, by each folder's `skill.toml` alone: a skill that does not load
/// just now keeps its function's name all the same.
fn tool_holder(skills: &Path, name: &str, tool: &str) -> Option<(String, SkillKind)> {
    let mut folders: Vec<(String, PathBuf)> = fs::read_dir(skills)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .filter(|(folder, path)| !folder.starts_with('.') && folder != name && path.is_dir())
        .collect();
    folders.sort();
    folders.into_iter().find_map(|(folder, path)| {
        let toml = toml_of(&path)?;
        (toml.get("tool").and_then(toml::Value::as_str) == Some(tool))
            .then(|| (folder, kind_of(&path)))
    })
}

/// What the enabled `skills` give the model: their functions, and what a
/// session does past its room (`docs/tools.md`). Inventory may include
/// disabled skills that still reserve a name but offer no function.
fn functions_said(skills: &[Skill]) -> String {
    // A skill's `[stop]` is one function more (#1685), and one `[stop]`
    // shared by several skills is one function, as the tools file has it
    // (the four wellbeing nudges' `wellbeing_nudges`).
    let mut tools: Vec<&str> = Vec::new();
    for tool in skills
        .iter()
        .filter(|skill| skill.enabled)
        .flat_map(|skill| {
            skill
                .tool
                .as_deref()
                .into_iter()
                .chain(skill.stop.as_ref().map(|stop| stop.tool.as_str()))
        })
    {
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    if tools.is_empty() {
        return "The skills here give the model no function.".into();
    }
    format!(
        "The skills here give the model {} function{} ({}). A session takes {SESSION_FUNCTIONS}, \
         the voice's own and the actions part's first (30 on a product lamp), and past \
         {SESSION_FUNCTIONS} the last are dropped unless the voice runs with LAMP_USE_SKILL=on \
         (every skill through one function, use_skill).",
        tools.len(),
        if tools.len() == 1 { "" } else { "s" },
        tools.join(", ")
    )
}

/// Installs the skill at `from`, a folder or a package (`.tar`), in the
/// skills folder `skills`, and says what was done. It is put together
/// beside the live skills where the running program does not look, loaded
/// there with every check a load makes (its permissions among them), and
/// only then moved in whole: the running program never sees half a skill,
/// and one that does not load is never live. A core skill is refused, and
/// so is one whose name a core skill on the lamp has, or whose tool's name
/// another skill there has. What a killed install or remove left is
/// deleted first ([`swept`]). With `state`, the running program's state
/// folder, the owner's yes to the camera kept for its name is taken away
/// before it goes in ([`moved_in`]).
pub fn install(skills: &Path, state: Option<&Path>, from: &Path) -> Result<String, String> {
    if !fs::metadata(skills).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("no skills folder at {}", skills.display()));
    }
    let _held = locked(skills)?;
    let swept = swept(skills);
    let meta = fs::metadata(from).map_err(|error| format!("{}: {error}", from.display()))?;
    let (name, files, package) = if meta.is_dir() {
        let from =
            fs::canonicalize(from).map_err(|error| format!("{}: {error}", from.display()))?;
        let name = from
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        (name, read_folder(&from)?, None)
    } else {
        let mut package = read_package(from)
            .map_err(|why| format!("not installed: {}: {why}", from.display()))?;
        let files = std::mem::take(&mut package.files);
        (package.manifest.name.clone(), files, Some(package))
    };
    put_in(
        skills,
        &swept,
        &name,
        &files,
        package.as_ref(),
        state,
        false,
        &[],
    )
}

/// A skill made on this lamp by voice (`lamp_voice::made_skill`, #1685),
/// put on at its person's spoken yes, which is bound to its package's
/// SHA-256 (the lead, 20 Oct 08:37 on #1711): the voice asks with that hash
/// alone. The package is the one the voice's made-skills folder `made`
/// names with that hash in its index; its bytes are read once and hashed,
/// and those same bytes are then read as a package, so what goes on is what
/// the person said yes to. Its `skill.toml` is held again to a made skill's
/// cap (`lamp_voice::made_skill::capped`: light, body, two topics) and its
/// `SKILL.md` must say `made: here`. Then it is installed as any package
/// is, the folder waited for at most `wait`.
pub fn install_made(
    skills: &Path,
    state: Option<&Path>,
    made: &Path,
    sha256: &str,
    wait: Duration,
) -> Result<String, String> {
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    install_made_at(skills, state, made, sha256, wait, now_unix_ms)
}

/// [`install_made`] at `now_unix_ms`: only the made skill the lamp asked
/// about last, within `lamp_voice::made_skill::PUT_ON_WITHIN` and not yet
/// on, as the voice's own `put_on` (the first builder on #2602, 22 Oct
/// 10:1x). A kept SHA-256 asked for later, or one not asked about at all,
/// puts nothing on, whoever asks. The time is the helper's, never the
/// asker's.
pub fn install_made_at(
    skills: &Path,
    state: Option<&Path>,
    made: &Path,
    sha256: &str,
    wait: Duration,
    now_unix_ms: u64,
) -> Result<String, String> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(
            "not installed: a made skill is asked for by its SHA-256, 64 lowercase hex".into(),
        );
    }
    let index: Vec<serde_json::Value> = fs::read(made.join(lamp_voice::made_skill::INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let Some(file) = index
        .iter()
        .find(|entry| entry["sha256"] == sha256)
        .and_then(|entry| entry["package"].as_str())
        .filter(|file| !file.is_empty() && !file.contains('/') && !file.starts_with('.'))
    else {
        return Err("not installed: no made skill is kept with that SHA-256".into());
    };
    if lamp_voice::made_skill::waiting(made, now_unix_ms).map(|(_, asked)| asked)
        != Some(sha256.to_owned())
    {
        return Err(format!(
            "not installed: the lamp didn't ask about that made skill last, within {} s",
            lamp_voice::made_skill::PUT_ON_WITHIN.as_secs()
        ));
    }
    let path = made.join(file);
    let meta =
        fs::symlink_metadata(&path).map_err(|error| format!("not installed: {file}: {error}"))?;
    if !meta.is_file() || meta.len() > MOST_PACKAGE_BYTES {
        return Err(format!("not installed: {file} is not a package"));
    }
    let mut bytes = Vec::new();
    open_plain(&path, &meta)
        .and_then(|opened| {
            opened
                .take(MOST_PACKAGE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())
        })
        .map_err(|why| format!("not installed: {file}: {why}"))?;
    if sha256_hex(&bytes) != sha256 {
        return Err(format!(
            "not installed: {file} is not what was said: its SHA-256 has changed"
        ));
    }
    let mut package =
        read_package_bytes(&bytes, None).map_err(|why| format!("not installed: {file}: {why}"))?;
    let card = |name: &str| {
        package
            .files
            .get(name)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    };
    lamp_voice::made_skill::capped(&card("skill.toml"))
        .map_err(|refused| format!("not installed: {}", refused.0))?;
    if !made_here(&card("SKILL.md")) {
        return Err("not installed: its SKILL.md doesn't say it was made here".into());
    }
    if !fs::metadata(skills).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("no skills folder at {}", skills.display()));
    }
    let name = package.manifest.name.clone();
    if !skill_name_ok(&name) {
        return Err(format!("not installed: {name:?} is not a skill's name"));
    }
    let _held = locked_within(skills, wait)?;
    // A person's yes to their own made skill never takes the place of a
    // skill they put on another way (Guard mode, Look for my keys, one from
    // a store) without a word (the reviewer's P3 on #2414): only one made
    // here is replaced, and the one in the way is named.
    let there = skills.join(&name);
    if there.exists()
        && !fs::read_to_string(there.join("SKILL.md")).is_ok_and(|card| made_here(&card))
    {
        return Err(format!(
            "not installed: {name} is already on the lamp, put on another way, and a skill made here doesn't take its place"
        ));
    }
    let swept = swept(skills);
    let files = std::mem::take(&mut package.files);
    // What binds these bytes is the person's yes, by their SHA-256, not a
    // signature (the reviewer's P3 on #2414).
    put_in(
        skills,
        &swept,
        &name,
        &files,
        Some(&package),
        state,
        false,
        &[],
    )
    .map(|said| {
        said.replace(
            NO_SIGNATURE,
            &format!(
                "bound to the person's yes by its SHA-256, {}",
                &sha256[..12]
            ),
        )
    })
}

/// Where `lamp-control` keeps the one push in flight on the lamp
/// (`docs/control.md`, "Skills pushed from a computer"): its state folder's
/// `push/`, [`PUSH_INDEX`] beside [`PUSH_PACKAGE`].
pub const PUSH_DIR: &str = "/var/lib/lampos-control/push";
/// The push in flight: its SHA-256, the computer, the skill's name, the
/// computer's grants and when it was taken.
pub const PUSH_INDEX: &str = "push.json";
/// The pushed package's bytes.
pub const PUSH_PACKAGE: &str = "package.tar";
/// A push is installed only within this long of `lamp-control` taking it.
pub const PUSH_WITHIN_MS: u64 = 60_000;
/// The most bytes of a pushed package (#3869): 1 MiB.
pub const MOST_PUSHED_BYTES: u64 = 1024 * 1024;

/// [`PUSH_INDEX`], as `lamp-control` writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Push {
    sha256: String,
    client: String,
    client_name: String,
    name: String,
    grants: Vec<String>,
    at_unix_ms: u64,
    /// The door's revision of that computer's pairing when it took the
    /// push: the order of a push and a change, from the door, not a clock.
    #[serde(default)]
    rev: Option<u64>,
    /// The grants the computer showed the person for this package
    /// (`grants_asked`'s names, in its order): required of a signed
    /// package, checked against the lamp's own list (Ten on #3931).
    #[serde(default)]
    asks: Option<Vec<String>>,
}

/// A pushed skill's [`super::PUSHED_FROM`]: who pushed which bytes, when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushedFrom {
    pub computer: String,
    pub client: String,
    pub sha256: String,
    pub at_unix_ms: u64,
    /// For a shared package, its author's whole key (base64 of the 32
    /// bytes): what an update of this name must be signed by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// That key's fingerprint (FPR16), for showing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_fingerprint: Option<String>,
}

/// What a push installed, as the door passes it on (#3869's `skill`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PushedDone {
    pub name: String,
    pub version: Option<String>,
    pub sha256: String,
    pub on: bool,
    pub held: Option<String>,
    pub asks: Vec<String>,
    pub shown: Vec<String>,
    /// A shared package's author, FPR16: "signed by key FPR16, not the
    /// Store", never "verified" (Fourteen's point 1 on #3504).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Said when the computer's `asks` aren't the lamp's own (`asks_differ`).
pub const ASKS_DIFFER: &str = "the permissions differ from those the computer showed";

/// Whether `signature`, a package's `lamp-skill.sig`, is an author's: it
/// carries their public key. A Store signature carries none, and makes
/// a plain push.
fn carries_author(signature: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(signature)
        .is_ok_and(|signed| signed.get("public").is_some())
}

/// What a pushed skill's refusal is called on the wire, for the door to
/// pass on (#3869's errors, Ten on #3931).
fn refusal_code(why: &str) -> Option<&'static str> {
    if why.contains(ASKS_DIFFER) {
        Some("asks_differ")
    } else if why.contains(": remove it first") {
        Some("signer_changed")
    } else if why.starts_with("not installed: its signature: ") {
        Some("bad_package")
    } else {
        no_grant(why).then_some("no_grant")
    }
}

/// What `held` says of a pushed skill that sees the picture.
pub const HELD_FOR_THE_PICTURE: &str = "sees the picture: off until the owner says yes in the app";

/// The protocol's grants a skill's permissions ask, each with the words the
/// owner's app shows for it, in the protocol's order (`docs/control.md`'s
/// grant table): the pairing screen's `Grant::shown()` in lamp-control says
/// the same of the computer, "can …" (Nine and Eighth on #3504). `camera` is the picture, never a grant a push carries.
pub fn grants_asked(skill: &Skill) -> Vec<(&'static str, &'static str)> {
    let permissions = &skill.permissions;
    // Format 2's `look` is presence alone (#3878), whatever topics it lists.
    let presence = (permissions.look && permissions.format_2)
        || permissions
            .topics
            .iter()
            .any(|topic| matches!(topic.as_str(), "humans/faces/tracked" | "room/stood_up"));
    [
        (permissions.body, ("body", "moves the lamp's body")),
        (permissions.light, ("light", "changes the light")),
        (permissions.speak, ("speak", "speaks and plays sounds")),
        (presence, ("look", "knows when someone is there, never who")),
        (
            permissions.sees_picture(),
            ("camera", "uses the camera when it runs"),
        ),
    ]
    .into_iter()
    .filter_map(|(asked, grant)| asked.then_some(grant))
    .collect()
}

/// The pushed skill of `push`'s one push in flight, put into the developer
/// folder `dev` (`docs/skills.md`, "Skills pushed from a computer"),
/// shaped as [`install_made`]: only the push `lamp-control` took last,
/// within [`PUSH_WITHIN_MS`], its bytes read once and hashed, loaded with
/// every check, held to the protocol's grants and to the computer's own,
/// never in the place of a skill in `skills` or of another computer's.
#[allow(clippy::too_many_arguments)]
pub fn install_pushed_at(
    skills: &Path,
    dev: &Path,
    push: &Path,
    store_key_file: &Path,
    sha256: &str,
    wait: Duration,
    now_unix_ms: u64,
) -> Result<PushedDone, String> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err("not installed: a push is asked for by its SHA-256, 64 lowercase hex".into());
    }
    let index = read_small(&push.join(PUSH_INDEX), ASKED_MOST)
        .map_err(|why| format!("not installed: no push is kept: {why}"))?;
    let kept: Push = serde_json::from_slice(&index)
        .map_err(|_| format!("not installed: {PUSH_INDEX} is not a push"))?;
    if kept.sha256 != sha256 {
        return Err("not installed: the push kept is not that SHA-256".into());
    }
    if kept.at_unix_ms > now_unix_ms || now_unix_ms - kept.at_unix_ms > PUSH_WITHIN_MS {
        return Err(format!(
            "not installed: that push was not taken within the last {} s",
            PUSH_WITHIN_MS / 1000
        ));
    }
    let bytes = read_small(&push.join(PUSH_PACKAGE), MOST_PUSHED_BYTES)
        .map_err(|why| format!("not installed: {PUSH_PACKAGE}: {why}"))?;
    if sha256_hex(&bytes) != sha256 {
        return Err(format!(
            "not installed: {PUSH_PACKAGE}'s SHA-256 has changed"
        ));
    }
    let mut package =
        read_package_bytes(&bytes, None).map_err(|why| format!("not installed: {why}"))?;
    // A shared package (its signature carries its author's key) is read
    // again with the author's check, against the Store's keys, read without
    // fail: an error there is a refusal, never an empty list (#3931, #3938).
    let author = match package.signature.as_deref() {
        Some(signature) if carries_author(signature) => {
            let store = store_keys(skills, store_key_file)
                .map_err(|why| format!("not installed: {why}"))?;
            let shared = read_shared_package_bytes(&bytes, &store)
                .map_err(|why| format!("not installed: its signature: {why}"))?;
            if kept.asks.is_none() {
                return Err(format!(
                    "not installed: {ASKS_DIFFER}: a signed package is pushed with the asks shown"
                ));
            }
            shared.author
        }
        _ => None,
    };
    let name = package.manifest.name.clone();
    if name != kept.name || !skill_name_ok(&name) {
        return Err(format!(
            "not installed: {name:?} is not the skill the push named"
        ));
    }
    if !fs::metadata(dev).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("no developer folder at {}", dev.display()));
    }
    if skills.join(&name).exists() {
        return Err(
            "not installed: a skill of that name is already on the lamp: a pushed skill never takes its place"
                .into(),
        );
    }
    let _held = locked_within(dev, wait)?;
    // Read under the folder's lock, as a revoke or a grant change writes
    // it: whichever the helper serves first, a push taken before a revoke
    // never lands, nor one asking a grant taken back since.
    // A record that can't be read stops every install, never none.
    // A change comes after the push when the door's revisions say so; when
    // either has none, by the time.
    let changed: Vec<Revoked> = revoked(dev)
        .map_err(|why| format!("not installed: {REVOKED}: {why}"))?
        .changes
        .into_iter()
        .filter(|change| {
            (change.client == kept.client || change.client == EVERY_COMPUTER)
                && match (change.rev, kept.rev) {
                    (Some(change), Some(push)) => change > push,
                    _ => change.at_unix_ms >= kept.at_unix_ms,
                }
        })
        .collect();
    if changed.iter().any(|change| change.grants.is_none()) {
        return Err("not installed: that computer was revoked after the push".into());
    }
    let author_key = author.as_ref().map(|author| STANDARD.encode(author.public));
    if let Some(from) = pushed_from(&dev.join(&name)) {
        if from.client != kept.client {
            return Err("not installed: another computer's skill has that name".into());
        }
        // A signer never changes in place: an update of a name signed by
        // one key comes signed by that key, and one pushed unsigned stays
        // unsigned, until it's removed (Ten on #3931).
        if from.author != author_key {
            return Err(match &from.author_fingerprint {
                Some(on) => format!(
                    "not installed: {name} is signed by key {on} on this lamp: remove it first"
                ),
                None => format!(
                    "not installed: {name} was pushed unsigned on this lamp: remove it first"
                ),
            });
        }
    }
    let files = std::mem::take(&mut package.files);
    let from = PushedFrom {
        computer: kept
            .client_name
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect(),
        client: kept.client.clone(),
        sha256: sha256.to_owned(),
        at_unix_ms: kept.at_unix_ms,
        author: author_key,
        author_fingerprint: author.as_ref().map(|author| author.fingerprint.clone()),
    };
    let mut from_json =
        serde_json::to_vec_pretty(&from).map_err(|error| format!("not installed: {error}"))?;
    from_json.push(b'\n');
    // Checked as the running program will load it, beside the skills
    // folder, before it is moved in: the protocol's grants, and never a
    // skill's name, tool or budget group there.
    let checking = hidden_place(dev, "check", &name);
    let checked = (|| -> Result<Skill, String> {
        let failed = |error: io::Error| format!("not installed: {}: {error}", checking.display());
        if checking.exists() {
            fs::remove_dir_all(&checking).map_err(failed)?;
        }
        let folder = checking.join(&name);
        for (path, bytes) in &files {
            let at = folder.join(path);
            if let Some(parent) = at.parent() {
                fs::create_dir_all(parent).map_err(failed)?;
            }
            fs::write(&at, bytes).map_err(failed)?;
        }
        let none = BTreeSet::new();
        let pushed = super::Pushed {
            dir: &checking,
            picture_yes: &none,
        };
        super::load_each_with(skills, Some(pushed))
            .into_iter()
            .find(|(path, _)| path == &folder)
            .map(|(_, loaded)| loaded)
            .unwrap_or_else(|| Err("it was not found where it was put".into()))
            .map_err(|why| format!("not installed: {why}"))
    })();
    let _ = fs::remove_dir_all(&checking);
    let skill = checked?;
    let asks = grants_asked(&skill);
    if let Some(shown) = &kept.asks
        && !shown
            .iter()
            .map(String::as_str)
            .eq(asks.iter().map(|(grant, _)| *grant))
    {
        return Err(format!(
            "not installed: {ASKS_DIFFER}: it asks [{}], and [{}] were shown",
            asks.iter()
                .map(|(grant, _)| *grant)
                .collect::<Vec<_>>()
                .join(", "),
            shown.join(", ")
        ));
    }
    if let Some((grant, _)) = asks
        .iter()
        .find(|(grant, _)| *grant != "camera" && !kept.grants.iter().any(|given| given == grant))
    {
        return Err(format!(
            "not installed: it asks {grant}, which this computer was not granted"
        ));
    }
    if let Some(grant) = changed
        .iter()
        .find_map(|change| beyond(&asks, change.grants.as_deref().unwrap_or_default()))
    {
        return Err(format!(
            "not installed: it asks {grant}, which this computer was not granted"
        ));
    }
    let swept = swept(dev);
    // No camera state and not from the Store: a pushed skill's picture yes
    // is kept per computer and SHA-256 in the developer folder, never in
    // the skills folder's camera record.
    put_in(
        dev,
        &swept,
        &name,
        &files,
        Some(&package),
        None,
        false,
        &[(super::PUSHED_FROM, from_json)],
    )?;
    let held = skill
        .permissions
        .sees_picture()
        .then(|| HELD_FOR_THE_PICTURE.to_owned());
    Ok(PushedDone {
        name,
        version: skill.version.clone(),
        sha256: sha256.to_owned(),
        on: skill.enabled && held.is_none(),
        held,
        asks: asks.iter().map(|(grant, _)| (*grant).to_owned()).collect(),
        shown: asks.iter().map(|(_, words)| (*words).to_owned()).collect(),
        author: author.map(|author| author.fingerprint),
    })
}

/// The computers revoked, or whose grants changed, each at the time its
/// `off_pushed_by` or `grants_pushed_by` came, in the developer folder (a
/// dot file): [`install_pushed_at`] refuses a push that computer made
/// before then, after a revoke, or asking a grant it no longer holds, so an
/// install already with another helper can't land the skill on after the
/// change (Fourteen's P2 on #3910, and Reviewer-two's on a re-pair with
/// fewer grants). Each is kept only while a push it could stop may still
/// be installed, at most [`MOST_REVOKED`] of them.
pub const REVOKED: &str = ".revoked.json";
const MOST_REVOKED: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Revoked {
    client: String,
    at_unix_ms: u64,
    /// The grants it holds from then, or none for a revoke.
    #[serde(default)]
    grants: Option<Vec<String>>,
    /// The door's revision of the change, when it sent one.
    #[serde(default)]
    rev: Option<u64>,
}

/// The record: the changes kept, and each computer's highest revision, kept
/// past the changes' two minutes, so an older change sent late is stale
/// however long after it comes (Reviewer-two on #3504).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Changes {
    #[serde(default)]
    changes: Vec<Revoked>,
    #[serde(default)]
    revs: BTreeMap<String, u64>,
}

/// The most computers whose highest revision is kept; past it, the
/// lowest go.
const MOST_REVS: usize = 256;

/// What a change of a computer's did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    /// Kept, and these of its skills turned off.
    Off(Vec<String>),
    /// Not after the change already kept for that computer, whose revision
    /// this is: nothing done (Reviewer-two on #3504, a reversed pair).
    Stale(u64),
}

/// The first of a skill's `asks` that `grants` lacks: the picture is the
/// owner's yes, never a pairing grant.
fn beyond<'a>(asks: &[(&'a str, &str)], grants: &[String]) -> Option<&'a str> {
    asks.iter()
        .map(|(grant, _)| *grant)
        .find(|grant| *grant != "camera" && !grants.iter().any(|given| given == grant))
}

/// A change kept for every computer: what stands for changes dropped to
/// keep the record small, or for a record that could not be read, so a
/// push made before it is refused as if its own computer's were kept
/// (Reviewer-two on #3504: the fence fails closed).
const EVERY_COMPUTER: &str = "*";

/// The changes kept, none when there is no record; an error when it can't
/// be read or isn't one.
fn revoked(dev: &Path) -> Result<Changes, String> {
    let path = dev.join(REVOKED);
    if fs::symlink_metadata(&path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Ok(Changes::default());
    }
    let bytes = read_small(&path, 64 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|_| "not a record of changes".to_owned())
}

/// A revoke of `client`, or its `skills` grant taken back, at `now_unix_ms`:
/// kept first, under the developer folder's lock, then every skill it
/// pushed turned off, kept for the owner to choose (#3869). The names
/// turned off.
pub fn off_pushed_by_at(
    dev: &Path,
    state: &Path,
    client: &str,
    rev: Option<u64>,
    wait: Duration,
    now_unix_ms: u64,
) -> Result<Changed, String> {
    changed_at(dev, state, client, None, rev, wait, now_unix_ms)
}

/// The owner's change of `client`'s grants to `grants`, at `now_unix_ms`
/// (a re-pair with fewer, or a grant taken back): kept first, as a revoke
/// is, then each skill it pushed that asks a grant outside them turned off,
/// kept for the owner to choose (#3869, "always held to the computer's
/// current grants"). The names turned off.
pub fn grants_pushed_by_at(
    dev: &Path,
    state: &Path,
    client: &str,
    grants: &[String],
    rev: Option<u64>,
    wait: Duration,
    now_unix_ms: u64,
) -> Result<Changed, String> {
    changed_at(dev, state, client, Some(grants), rev, wait, now_unix_ms)
}

fn changed_at(
    dev: &Path,
    state: &Path,
    client: &str,
    grants: Option<&[String]>,
    rev: Option<u64>,
    wait: Duration,
    now_unix_ms: u64,
) -> Result<Changed, String> {
    {
        let _held = locked_within(dev, wait)?;
        // Every change still able to stop a push is kept, a computer's
        // earlier ones too: a re-pair after a revoke never lifts the revoke
        // for a push made before it.
        // One that can't be read is replaced by a revoke of every computer
        // now, so nothing it held is lost.
        let mut record = revoked(dev).unwrap_or_else(|why| {
            eprintln!(
                "lamp-skills: asked: {REVOKED}: {why}: every computer's older pushes refused"
            );
            Changes {
                changes: vec![Revoked {
                    client: EVERY_COMPUTER.to_owned(),
                    at_unix_ms: now_unix_ms,
                    grants: None,
                    rev: None,
                }],
                revs: BTreeMap::new(),
            }
        });
        if let Some(rev) = rev {
            match record.revs.get(client) {
                Some(&highest) if highest >= rev => return Ok(Changed::Stale(highest)),
                _ => {
                    record.revs.insert(client.to_owned(), rev);
                }
            }
            while record.revs.len() > MOST_REVS {
                let lowest = record
                    .revs
                    .iter()
                    .min_by_key(|(_, rev)| **rev)
                    .map(|(client, _)| client.clone());
                if let Some(lowest) = lowest {
                    record.revs.remove(&lowest);
                }
            }
        }
        let kept = &mut record.changes;
        kept.push(Revoked {
            client: client.to_owned(),
            at_unix_ms: now_unix_ms,
            grants: grants.map(<[String]>::to_vec),
            rev,
        });
        kept.sort_by_key(|change| change.at_unix_ms);
        // Those past two minutes, and past the most kept the oldest, go,
        // and a revoke of every computer at the newest of them stands in
        // their place: a push made before it is refused still.
        let old = kept
            .iter()
            .take_while(|change| now_unix_ms.saturating_sub(change.at_unix_ms) > 2 * PUSH_WITHIN_MS)
            .count();
        let dropped = old.max((kept.len() + 1).saturating_sub(MOST_REVOKED));
        if dropped > 0 {
            let floor = kept[..dropped]
                .iter()
                .map(|change| change.at_unix_ms)
                .max()
                .unwrap_or(now_unix_ms);
            kept.drain(..dropped);
            kept.insert(
                0,
                Revoked {
                    client: EVERY_COMPUTER.to_owned(),
                    at_unix_ms: floor,
                    grants: None,
                    rev: None,
                },
            );
        }
        let mut json = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
        json.push(b'\n');
        let path = dev.join(REVOKED);
        lamp_update::records::write_whole(&path, &json, 0o644)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let mut off = Vec::new();
    for (name, _) in pushed_by_client(dev, client) {
        let asks_more = match grants {
            None => true,
            // One that no longer loads is turned off too: what it asks is
            // not known.
            Some(grants) => super::load(&dev.join(&name)).map_or(true, |skill| {
                beyond(&grants_asked(&skill), grants).is_some()
            }),
        };
        if asks_more {
            turn(dev, state, &name, true, wait)?;
            off.push(name);
        }
    }
    Ok(Changed::Off(off))
}

/// Whether a refused install is a grant's, answered with `"code":
/// "no_grant"` so lamp-control gives the computer `no_grant` (#3869's
/// errors): a permission beyond the protocol's, a topic past presence
/// (`hear`, `sound`), or a grant this computer was not given.
fn no_grant(why: &str) -> bool {
    why.contains("a pushed skill may not ")
        || why.ends_with(", which this computer was not granted")
}

/// The owner's yeses to pushed skills seeing the picture, in the developer
/// folder (a dot file): each for a name, the computer that pushed it and
/// the SHA-256 of what it pushed, so a re-push, or another computer's push
/// of that name, has none (the Reviewer-lead's P3 on #3879).
pub const PICTURE_YES: &str = ".yes.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Yes {
    name: String,
    client: String,
    sha256: String,
    /// A shared skill's author's key: another signer is no yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    author: Option<String>,
}

fn yeses(dev: &Path) -> Vec<Yes> {
    read_small(&dev.join(PICTURE_YES), 64 * 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The pushed skills in `dev` the owner said yes to seeing the picture,
/// each yes still for the computer and bytes now there.
pub fn picture_yes(dev: &Path) -> BTreeSet<String> {
    yeses(dev)
        .into_iter()
        .filter(|yes| {
            skill_name_ok(&yes.name)
                && pushed_from(&dev.join(&yes.name)).is_some_and(|from| {
                    from.client == yes.client
                        && from.sha256 == yes.sha256
                        && from.author == yes.author
                })
        })
        .map(|yes| yes.name)
        .collect()
}

/// The owner's yes to the pushed skill `name` as it is now, kept whole.
fn said_yes(dev: &Path, name: &str, wait: Duration) -> Result<(), String> {
    if !skill_name_ok(name) {
        return Err(format!("{name:?} is not a skill's name"));
    }
    let _held = locked_within(dev, wait)?;
    let from = pushed_from(&dev.join(name)).ok_or_else(|| format!("no pushed skill {name}"))?;
    let mut kept: Vec<Yes> = yeses(dev)
        .into_iter()
        .filter(|yes| yes.name != name && dev.join(&yes.name).exists())
        .collect();
    kept.push(Yes {
        name: name.to_owned(),
        client: from.client,
        sha256: from.sha256,
        author: from.author,
    });
    let mut json = serde_json::to_vec_pretty(&kept).map_err(|error| error.to_string())?;
    json.push(b'\n');
    let path = dev.join(PICTURE_YES);
    lamp_update::records::write_whole(&path, &json, 0o644)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// A pushed skill folder's [`super::PUSHED_FROM`], read never through a
/// link and at most 1 KiB.
pub fn pushed_from(folder: &Path) -> Option<PushedFrom> {
    read_small(&folder.join(super::PUSHED_FROM), 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// A plain file's bytes, never through a link, at most `most`.
fn read_small(path: &Path, most: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.len() > most {
        return Err(format!("not a plain file of at most {most} bytes"));
    }
    let mut bytes = Vec::new();
    open_plain(path, &meta)?
        .take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > most {
        return Err(format!("more than {most} bytes"));
    }
    Ok(bytes)
}

/// The developer folder and lamp-control's push, for the manage helper.
#[derive(Clone, Copy)]
pub struct Pushes<'a> {
    pub dev: &'a Path,
    pub push: &'a Path,
    /// Who connected, by its cgroup ([`peer_asker`]): only lamp-control
    /// asks the pushed requests, and lamp-control asks nothing else
    /// (Reviewer-two on #3504).
    pub asker: Asker,
    /// The Store's public key, which a shared package may not carry
    /// ([`store_keys`], with the build's beside the skills folder first).
    pub key_file: &'a Path,
}

/// Which part asked the manage helper, by its unit ([`asker_of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// `lamp-cloud` (the owner's app) or the voice: the Store's requests,
    /// made skills and the app's off and on.
    App,
    /// `lamp-control`, a paired computer's door: the pushed requests only.
    Control,
    /// Any peer, with [`ANY_ASKER`] on, as in the tests.
    Any,
}

/// `lamp-control`, where the system's manager puts it.
pub const CONTROL: &str = "/lampos.slice/lampos-parts.slice/lamp-control.service";

/// Why a request isn't taken from the part that asked it.
pub const NOT_ITS_REQUEST: &str =
    "only lamp-control asks for pushed skills, and it asks for nothing else";

/// `unix_ms` as UTC, `2026-10-30T06:22:45Z`: a pushed skill's `pushed_at`
/// in `list_pushed_by`'s answer, as control.md shows it.
fn utc_text(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The manage helper's requests for skills pushed from a computer, each
/// one key: `{"install_pushed": SHA256}`, `{"off_pushed_by": CLIENT}`,
/// `{"list_pushed_by": CLIENT}` and `{"remove_pushed": {"client", "name"}}`.
pub const PUSHED_REQUESTS: [&str; 5] = [
    "install_pushed",
    "off_pushed_by",
    "grants_pushed_by",
    "list_pushed_by",
    "remove_pushed",
];

/// The answer to a pushed request on a helper given no developer folder.
pub const PUSH_OFF: &str = "skill push is not on this lamp";

/// A change's `rev`, if it has one: a whole number.
fn revision(value: &serde_json::Value) -> Result<Option<u64>, String> {
    match value.get("rev") {
        None => Ok(None),
        Some(rev) => rev
            .as_u64()
            .map(Some)
            .ok_or_else(|| "a change's rev is a whole number".to_owned()),
    }
}

/// A change's answer: `{"off": [...]}`, or `{"stale": REV}` with the
/// computer's revision already kept.
fn said_changed(changed: Changed) -> serde_json::Value {
    match changed {
        Changed::Off(off) => serde_json::json!({ "off": off }),
        Changed::Stale(rev) => serde_json::json!({ "stale": rev }),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// A paired computer's id as lamp-control gives it: `c_` and up to 64
/// lowercase letters, digits and `_`.
fn client_ok(client: &str) -> bool {
    client.len() <= 66
        && client.strip_prefix("c_").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// The skills in `dev` that `client` pushed: each folder's name, and what
/// its [`super::PUSHED_FROM`] says.
fn pushed_by_client(dev: &Path, client: &str) -> Vec<(String, PushedFrom)> {
    let mut found: Vec<(String, PushedFrom)> = fs::read_dir(dev)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') || !entry.path().is_dir() {
                return None;
            }
            let from = pushed_from(&entry.path())?;
            (from.client == client).then_some((name, from))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn pushed_request(
    skills: &Path,
    state: &Path,
    pushes: Pushes<'_>,
    key: &str,
    value: &serde_json::Value,
    wait: Duration,
) -> Result<serde_json::Value, String> {
    let client = |value: &serde_json::Value| {
        value
            .as_str()
            .filter(|client| client_ok(client))
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} takes a computer's id, as \"c_…\""))
    };
    match key {
        "install_pushed" => {
            let sha256 = value
                .as_str()
                .ok_or("a push is asked for as {\"install_pushed\": SHA256}")?;
            install_pushed_at(
                skills,
                pushes.dev,
                pushes.push,
                pushes.key_file,
                sha256,
                wait,
                now_unix_ms(),
            )
            .and_then(|done| serde_json::to_value(done).map_err(|error| error.to_string()))
        }
        // A plain id, from a door that sends no revision yet, or
        // {"client", "rev"}.
        "off_pushed_by" => {
            let (asker, rev) = match value.get("client") {
                Some(asker) => (client(asker)?, revision(value)?),
                None => (client(value)?, None),
            };
            off_pushed_by_at(pushes.dev, state, &asker, rev, wait, now_unix_ms()).map(said_changed)
        }
        "grants_pushed_by" => {
            let said = "grants are changed as {\"grants_pushed_by\": {\"client\", \"rev\", \"grants\": [...]}}";
            let asker = client(value.get("client").ok_or(said)?)?;
            let rev = revision(value)?;
            let grants: Vec<String> = value
                .get("grants")
                .and_then(serde_json::Value::as_array)
                .ok_or(said)?
                .iter()
                .map(|grant| grant.as_str().map(str::to_owned).ok_or(said))
                .collect::<Result<_, _>>()?;
            if grants.len() > 16 {
                return Err(said.into());
            }
            grants_pushed_by_at(pushes.dev, state, &asker, &grants, rev, wait, now_unix_ms())
                .map(said_changed)
        }
        "list_pushed_by" => {
            let client = client(value)?;
            let offs = super::read_names(&state.join(super::OFF_FILE)).unwrap_or_default();
            let yes = picture_yes(pushes.dev);
            let listed: Vec<serde_json::Value> = pushed_by_client(pushes.dev, &client)
                .into_iter()
                .map(|(name, from)| {
                    let skill = super::load(&pushes.dev.join(&name)).ok();
                    let held = skill
                        .as_ref()
                        .is_some_and(|skill| {
                            skill.permissions.sees_picture() && !yes.contains(&name)
                        })
                        .then_some(HELD_FOR_THE_PICTURE);
                    let on = skill.as_ref().is_some_and(|skill| skill.enabled)
                        && held.is_none()
                        && !offs.contains(&name);
                    serde_json::json!({
                        "name": name,
                        "version": skill.and_then(|skill| skill.version),
                        "sha256": from.sha256,
                        "on": on,
                        "held": held,
                        "pushed_at": utc_text(from.at_unix_ms),
                        "pushed_at_unix_ms": from.at_unix_ms,
                        "author": from.author_fingerprint,
                    })
                })
                .collect();
            // The list itself: lamp-control answers it as `skills`
            // (docs/control.md, piece 7). Wrapped here too, a client got
            // `{"skills": {"skills": [...]}}` (lamp-75c9, 30 Oct 13:23).
            Ok(serde_json::Value::Array(listed))
        }
        "remove_pushed" => {
            let (Some(asker), Some(name)) = (
                value.get("client").and_then(serde_json::Value::as_str),
                value.get("name").and_then(serde_json::Value::as_str),
            ) else {
                return Err(
                    "a push is removed as {\"remove_pushed\": {\"client\", \"name\"}}".into(),
                );
            };
            let asker = client(&serde_json::Value::from(asker))?;
            if !skill_name_ok(name) {
                return Err(format!("{name:?} is not a skill's name"));
            }
            match pushed_from(&pushes.dev.join(name)) {
                Some(from) if from.client == asker => taken_off(pushes.dev, None, name, Some(wait))
                    .map(|left| {
                        if let Some(left) = left {
                            eprintln!("lamp-skills: asked: removed {name}, but {left}");
                        }
                        serde_json::Value::from(removed_said(name))
                    }),
                Some(_) => Err("not removed: another computer pushed that skill".into()),
                None => Err(format!("no skill {name} pushed from this computer")),
            }
        }
        _ => Err(NOT_A_REQUEST.to_owned()),
    }
}

/// What an install's done line says of a package with no signature.
const NO_SIGNATURE: &str = "it carries no signature: a package installed by hand from a file";

/// Whether a skill's `SKILL.md` says it was made on this lamp by voice
/// (`made: here` in its front matter, `lamp_voice::made_skill`).
fn made_here(card: &str) -> bool {
    card.lines().any(|line| line.trim() == "made: here")
}

/// The skill `name` of these `files` put together beside the live skills,
/// loaded and moved in ([`moved_in`]), with the skills folder [`locked`] by
/// the caller and what a killed run left `swept`. `state` is where the
/// owner's yes to the camera is kept: `from_store`, a Store package keeps
/// its name's yes (and needs it to ask the camera); any other install
/// takes it away ([`moved_in`]).
// Each is its own caller's: where, what, the camera's record, and the dot
// files beside it (a pushed skill's), none of them the same for all.
#[allow(clippy::too_many_arguments)]
fn put_in(
    skills: &Path,
    swept: &[String],
    name: &str,
    files: &Files,
    package: Option<&Package>,
    state: Option<&Path>,
    from_store: bool,
    beside: &[(&str, Vec<u8>)],
) -> Result<String, String> {
    let name = name.to_owned();
    if !skill_name_ok(&name) {
        return Err(format!(
            "not installed: {name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let staged = hidden_place(skills, "install", &name);
    let made = (|| -> Result<String, String> {
        let failed = |error: io::Error| format!("not installed: {}: {error}", staged.display());
        if staged.exists() {
            fs::remove_dir_all(&staged).map_err(failed)?;
        }
        let folder = staged.join(&name);
        for (path, bytes) in files {
            let at = folder.join(path);
            if let Some(parent) = at.parent() {
                fs::create_dir_all(parent).map_err(failed)?;
            }
            fs::write(&at, bytes).map_err(failed)?;
        }
        fs::create_dir_all(&folder).map_err(failed)?;
        if let Some(package) = package {
            let installed = Installed {
                sha256: package.sha256.clone(),
                manifest: package.manifest.clone(),
            };
            let mut json = serde_json::to_vec_pretty(&installed)
                .map_err(|error| format!("not installed: {error}"))?;
            json.push(b'\n');
            fs::write(folder.join(INSTALLED_FROM), json).map_err(failed)?;
        }
        // Dot files the skill comes with on this lamp, never from its
        // package (a pushed skill's [`super::PUSHED_FROM`]).
        for (file, bytes) in beside {
            fs::write(folder.join(file), bytes).map_err(failed)?;
        }
        open_to_parts(&folder).map_err(failed)?;
        moved_in(
            skills,
            &name,
            &folder,
            files.len(),
            package,
            state,
            from_store,
        )
    })();
    let _ = fs::remove_dir_all(&staged);
    if swept.is_empty() {
        made
    } else {
        made.map(|said| {
            format!(
                "{said} Deleted what a killed install or remove left: {}.",
                swept.join(", ")
            )
        })
    }
}

/// The skill put together in `folder` loaded, checked against what it
/// replaces, and moved into `skills`.
fn moved_in(
    skills: &Path,
    name: &str,
    folder: &Path,
    count: usize,
    package: Option<&Package>,
    state: Option<&Path>,
    from_store: bool,
) -> Result<String, String> {
    let skill =
        load(folder).map_err(|why| format!("not installed: {name} does not load: {why}"))?;
    if skill.kind == SkillKind::Core {
        return Err(format!(
            "not installed: {name} says kind = \"core\": a core skill comes with the lamp (lamp-update install), \
             never from a folder or a package"
        ));
    }
    if let Some(package) = package
        && skill.version.as_deref() != Some(package.manifest.version.as_str())
    {
        return Err(format!(
            "not installed: {MANIFEST} says version {}, and its skill.toml {}",
            package.manifest.version,
            skill.version.as_deref().unwrap_or("none")
        ));
    }
    let target = skills.join(name);
    let replaced = match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("not installed: {}: {error}", target.display())),
        Ok(meta) if !meta.is_dir() => {
            return Err(format!(
                "not installed: {} is there and is not a skill's folder: left as it is",
                target.display()
            ));
        }
        Ok(_) if kind_of(&target) == SkillKind::Core => {
            return Err(format!(
                "not installed: {name} is a core skill on this lamp: it comes with the lamp, \
                 and an installable skill can't take its place"
            ));
        }
        Ok(_) => Some(version_of(&target)),
    };
    // A package from the Store never puts an older version over a newer
    // one: an old package the channel once signed can't be replayed onto a
    // lamp (the reviewer-lead on #1983). The same version is a reinstall.
    if let Some(package) = package
        && package.signed_by.is_some()
        && let Some(installed) = &replaced
        && let Some(why) = older_refused(name, installed.as_deref(), &package.manifest.version)
    {
        return Err(format!("not installed: {why}"));
    }
    // A function is one skill's: one that takes another's name would take
    // it from the model, a core skill's first ([`load_each`]).
    if let Some(tool) = &skill.tool
        && let Some((holder, kind)) = tool_holder(skills, name, tool)
    {
        return Err(format!(
            "not installed: {name}'s tool {tool:?} is the {} skill {holder}'s on this lamp: \
             a function the model calls is one skill's",
            kind.name()
        ));
    }
    // The owner's yes to the camera is kept by name (#3504's piece C).
    // From the Store, a package that asks the camera's picture goes on only
    // once that yes is kept, and an update of the same catalogue entry
    // keeps it. Any other install that puts a skill in place under that
    // name (by hand, or made here) takes the yes away first, under the
    // same lock and before the rename, so its picture waits for a new yes
    // at each call (`Engine::picture_refused`); a record that can't be
    // read or written installs nothing.
    if let Some(state) = state {
        let file = state.join(super::CAMERA_FILE);
        let failed = |error: io::Error| format!("not installed: {}: {error}", file.display());
        let mut yes = super::read_names(&file).map_err(failed)?;
        if from_store {
            if skill.permissions.sees_picture() && !yes.contains(name) {
                return Err(format!(
                    "not installed: {name}: it asks the camera, and the owner hasn't said yes"
                ));
            }
        } else if yes.remove(name) {
            super::keep_names(&file, &yes).map_err(failed)?;
        }
    }
    let failed = |error: io::Error| format!("not installed: {}: {error}", target.display());
    match &replaced {
        None => fs::rename(folder, &target).map_err(failed)?,
        Some(_) => {
            // The old out of the way, the new in its place, at once: the
            // program looks every 2 s, and between the two it may see none.
            let old = hidden_place(skills, "removed", name);
            if old.exists() {
                fs::remove_dir_all(&old).map_err(failed)?;
            }
            fs::rename(&target, &old).map_err(failed)?;
            if let Err(error) = fs::rename(folder, &target) {
                let _ = fs::rename(&old, &target);
                return Err(failed(error));
            }
            let _ = fs::remove_dir_all(&old);
        }
    }
    let version = skill.version.as_deref().unwrap_or("with no version");
    let what = match replaced {
        None => format!("installed {name} {version} (installable)"),
        Some(before) => format!(
            "replaced {name} {} with {version} (installable)",
            before.as_deref().unwrap_or("with no version")
        ),
    };
    let from = match package {
        None => format!("from its folder, {count} files"),
        Some(package) => format!(
            "from its package, {count} files, each one's size and sha256 as {MANIFEST} says; {}",
            match (&package.signed_by, package.signature.is_some()) {
                (Some(key), _) => format!("signed with the Store's key {key}"),
                (None, true) => {
                    "its signature not checked: a package installed by hand from a file".to_owned()
                }
                (None, false) => NO_SIGNATURE.to_owned(),
            }
        ),
    };
    let (loaded, _) = super::load_all(skills);
    Ok(format!(
        "{what} {from}. It may use: {}. A running lamp-skills loads it within 2 s. {}",
        skill.permissions.said(),
        functions_said(&loaded)
    ))
}

/// Removes the installable skill `name` from `skills` and says so. A core
/// skill is refused: it comes with the lamp. With `state`, the running
/// program's state folder, the owner's yes to its camera is taken back
/// first ([`super::CAMERA_FILE`]), so a skill put on again by that name
/// asks again.
pub fn remove(skills: &Path, state: Option<&Path>, name: &str) -> Result<String, String> {
    match taken_off(skills, state, name, None)? {
        None => Ok(removed_said(name)),
        Some(left) => Err(format!("removed {name}, but {left}")),
    }
}

/// What a remove says once the skill is gone for the running program.
fn removed_said(name: &str) -> String {
    format!("removed {name}: a running lamp-skills lets it go within 2 s")
}

/// The running program's state folder on a product lamp (its unit's
/// `--state-dir`), where the skills turned off and the core skills turned
/// on are kept ([`super::OFF_FILE`], [`super::ON_FILE`]), and the owner's
/// yes to a skill's camera ([`super::CAMERA_FILE`]).
pub const STATE_DIR: &str = "/var/lib/lampos-skills-state";

/// The owner's yes to the skill `name` seeing the camera's picture kept,
/// or with `allow` false taken back, in `state`'s [`super::CAMERA_FILE`]
/// (#3504's piece C), written whole by [`super::keep_names`] under the
/// skills folder's lock (waited for at most `wait`), as [`turn`] keeps off
/// and on. The skill need not be on the lamp yet: the Store installs a
/// package that asks the camera only once its yes is kept here
/// ([`install_from_url`]). The running program reads the file at each
/// call that would give the picture ([`super::Engine::picture_refused`]).
pub fn camera(
    skills: &Path,
    state: &Path,
    name: &str,
    allow: bool,
    wait: Duration,
) -> Result<String, String> {
    if !skill_name_ok(name) {
        return Err(format!(
            "{name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let _held = locked_within(skills, wait)?;
    let file = state.join(super::CAMERA_FILE);
    let failed = |error: io::Error| format!("{}: {error}", file.display());
    let mut yes = super::read_names(&file).map_err(failed)?;
    let changed = if allow {
        yes.insert(name.to_owned())
    } else {
        yes.remove(name)
    };
    if changed {
        super::keep_names(&file, &yes).map_err(failed)?;
    }
    Ok(if allow {
        format!("{name} may see the camera's picture")
    } else {
        format!("{name} may not see the camera's picture")
    })
}

/// Why the skill `name`, granted `permissions`, can't be turned off, or
/// nothing: one granted a core operation ([`super::CORE_GRANTS`]: night
/// sleep, onboarding, prepare-to-unplug, the startle) is part of the lamp itself. The
/// one rule, for the app's page and for [`turn`].
pub fn cannot_turn_off(name: &str, permissions: &super::Permissions) -> Option<String> {
    (!permissions.core.is_empty())
        .then(|| format!("{name} is part of how the lamp itself works, so it can't be turned off"))
}

/// The skill `name` in `skills` turned off or on for the app (#2815), as
/// a `[stop]`'s off and on keep it: in `state`'s [`super::OFF_FILE`],
/// written whole by the same writer ([`super::keep_names`]), which the
/// running program reads again within 2 s and at a start, and which a
/// round never touches. A core skill shipped off is turned on in its own
/// `skill.toml`, as by hand, and kept in [`super::ON_FILE`] so a round
/// keeps it on (#2585); turned off again, its name leaves `on`, its file
/// is off again, and it is in `off`. A skill that is part of the lamp
/// itself is refused ([`cannot_turn_off`]), and so is one that does not
/// load. The folder's lock is waited for at most `wait`.
pub fn turn(
    skills: &Path,
    state: &Path,
    name: &str,
    off: bool,
    wait: Duration,
) -> Result<String, String> {
    if !skill_name_ok(name) {
        return Err(format!(
            "{name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let _held = locked_within(skills, wait)?;
    let skill = load_each(skills)
        .into_iter()
        .find(|(folder, _)| folder.file_name() == Some(std::ffi::OsStr::new(name)))
        .ok_or_else(|| format!("no skill {name} on this lamp"))?
        .1
        .map_err(|why| format!("{name} does not load, so it can't be turned off or on: {why}"))?;
    let failed = |path: &Path, error: io::Error| format!("{}: {error}", path.display());
    let off_file = state.join(super::OFF_FILE);
    let on_file = state.join(super::ON_FILE);
    let toml = skills.join(name).join("skill.toml");
    let mut offs = super::read_names(&off_file).map_err(|error| failed(&off_file, error))?;
    let mut ons = super::read_names(&on_file).map_err(|error| failed(&on_file, error))?;
    let rewrite = |text: String| {
        lamp_update::records::write_whole(&toml, text.as_bytes(), 0o644)
            .map_err(|error| failed(&toml, error))
    };
    if off {
        if let Some(why) = cannot_turn_off(name, &skill.permissions) {
            return Err(why);
        }
        if ons.remove(name) {
            if skill.enabled {
                let text = fs::read_to_string(&toml).map_err(|error| failed(&toml, error))?;
                rewrite(turned_off(&text))?;
            }
            super::keep_names(&on_file, &ons).map_err(|error| failed(&on_file, error))?;
        }
        if offs.insert(name.to_owned()) {
            super::keep_names(&off_file, &offs).map_err(|error| failed(&off_file, error))?;
        }
        return Ok(format!("turned {name} off"));
    }
    if offs.remove(name) {
        super::keep_names(&off_file, &offs).map_err(|error| failed(&off_file, error))?;
    }
    if !skill.enabled {
        if skill.kind == SkillKind::Core && ons.insert(name.to_owned()) {
            super::keep_names(&on_file, &ons).map_err(|error| failed(&on_file, error))?;
        }
        let text = fs::read_to_string(&toml).map_err(|error| failed(&toml, error))?;
        rewrite(lamp_update::product_skills::turned_on(&text))?;
    }
    Ok(format!("turned {name} on"))
}

/// A `skill.toml` turned on by [`lamp_update::product_skills::turned_on`]
/// turned off again: a top-level `enabled = true` made `false`, or, with
/// none, install's off line above it, so the build's own file comes back.
fn turned_off(toml: &str) -> String {
    let mut tables = false;
    let mut found = false;
    let mut out = String::with_capacity(toml.len());
    for line in toml.split_inclusive('\n') {
        tables |= line.trim_start().starts_with('[');
        if !tables
            && !found
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "enabled"
            && value.trim() == "true"
        {
            found = true;
            out.push_str(&line.replacen("true", "false", 1));
        } else {
            out.push_str(line);
        }
    }
    if found {
        out
    } else {
        format!("{}{toml}", lamp_update::install::OFF_LINE)
    }
}

/// [`remove`]'s steps, the folder's lock waited for at most `wait` (always,
/// when `None`). Once the skill is renamed aside it is gone for the running
/// program: `Some` says where it was set aside and why that was not then
/// deleted (the next install deletes it, [`left_by_a_run`]).
fn taken_off(
    skills: &Path,
    state: Option<&Path>,
    name: &str,
    wait: Option<Duration>,
) -> Result<Option<String>, String> {
    if !skill_name_ok(name) {
        return Err(format!(
            "{name:?} is not a skill's name: 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    let target = skills.join(name);
    match fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "{} is not a skill's folder: left as it is",
                target.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("no skill {name} on this lamp"));
        }
        Err(error) => return Err(format!("{}: {error}", target.display())),
    }
    let _held = match wait {
        None => locked(skills)?,
        Some(wait) => locked_within(skills, wait)?,
    };
    if kind_of(&target) == SkillKind::Core {
        return Err(format!(
            "{name} is a core skill: it comes with the lamp and can't be removed"
        ));
    }
    // The owner's yes to its camera goes with it, under the same lock and
    // before the folder: a skill put on again by this name, from the Store
    // or by hand, never inherits it (#3504's piece C). Not taken back,
    // nothing is removed.
    if let Some(state) = state {
        let file = state.join(super::CAMERA_FILE);
        let failed = |error: io::Error| format!("not removed: {}: {error}", file.display());
        let mut yes = super::read_names(&file).map_err(failed)?;
        if yes.remove(name) {
            super::keep_names(&file, &yes).map_err(failed)?;
        }
    }
    // Gone at once for the running program, then deleted.
    let old = hidden_place(skills, "removed", name);
    let failed = |error: io::Error| format!("not removed: {}: {error}", target.display());
    if old.exists() {
        fs::remove_dir_all(&old).map_err(failed)?;
    }
    fs::rename(&target, &old).map_err(failed)?;
    Ok(fs::remove_dir_all(&old)
        .err()
        .map(|error| format!("{} was not deleted: {error}", old.display())))
}

/// What is said of a URL that is not one a package is fetched from: the URL
/// is not said back.
pub const NOT_A_STORE_URL: &str = "url must be an https URL of at most 2048 characters, \
    with no spaces, no user and nothing but plain ASCII";
/// What is said of a `sha256` that is not one.
pub const NOT_A_SHA256: &str = "sha256 must be 64 lowercase hex characters";

/// Whether `url` is one a package is fetched from: `https://`, a host, at
/// most [`MOST_URL`] bytes of printable ASCII with no space and no user
/// (`@` before the path). Nothing else, `file://` and `http://` least of
/// all, is fetched, and the fetch follows a redirect only to https.
pub fn store_url_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    url.len() <= MOST_URL
        && !authority.is_empty()
        && !authority.contains('@')
        && url.bytes().all(|byte| byte.is_ascii_graphic())
}

/// A package asked for from the Skills Store: its URL, and what the
/// catalogue's entry says of it (the first builder's 22:23 on #1722: the
/// catalogue is a signed list of entries `{name, version, sha256, url}`),
/// each checked against what is fetched. `skills.catalog` will hand an
/// entry straight to an install; a bare URL is taken too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked<'a> {
    pub url: &'a str,
    /// The SHA-256 of the package's bytes, checked before they are read.
    pub sha256: Option<&'a str>,
    /// The skill's name and version, checked against its manifest.
    pub name: Option<&'a str>,
    pub version: Option<&'a str>,
}

/// What is said of a `name` that is not a skill's.
pub const NOT_A_NAME: &str =
    "name must be a skill's name: 1 to 64 lowercase letters, digits and hyphens";
/// What is said of a `version` that is not one.
pub const NOT_A_VERSION: &str = "version must be MAJOR.MINOR.PATCH";

impl<'a> Asked<'a> {
    /// A request's fields, `{"url", "sha256", "name", "version"}`, every one
    /// a string and all but `url` optional, nothing else: or none.
    pub fn from_json(asked: &'a serde_json::Value) -> Option<Self> {
        let asked = asked.as_object()?;
        if !asked
            .keys()
            .all(|key| matches!(key.as_str(), "url" | "sha256" | "name" | "version"))
        {
            return None;
        }
        let text = |key: &str| match asked.get(key) {
            None => Some(None),
            Some(value) => value.as_str().map(Some),
        };
        Some(Self {
            url: asked.get("url")?.as_str()?,
            sha256: text("sha256")?,
            name: text("name")?,
            version: text("version")?,
        })
    }

    /// Why it is not one a package is fetched for, if it is not.
    pub fn refused(&self) -> Option<&'static str> {
        if !store_url_ok(self.url) {
            Some(NOT_A_STORE_URL)
        } else if self.sha256.is_some_and(|sha256| !sha256_ok(sha256)) {
            Some(NOT_A_SHA256)
        } else if self.name.is_some_and(|name| !skill_name_ok(name)) {
            Some(NOT_A_NAME)
        } else if self.version.is_some_and(|version| !version_ok(version)) {
            Some(NOT_A_VERSION)
        } else {
            None
        }
    }

    /// The request as the manage helper takes it, `{"install": {...}}`.
    pub fn request(&self) -> serde_json::Value {
        let mut fields = serde_json::Map::new();
        fields.insert("url".into(), self.url.into());
        for (key, value) in [
            ("sha256", self.sha256),
            ("name", self.name),
            ("version", self.version),
        ] {
            if let Some(value) = value {
                fields.insert(key.into(), value.into());
            }
        }
        serde_json::json!({ "install": fields })
    }
}

/// Whether `text` is a SHA-256 as this program writes one.
pub fn sha256_ok(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The Store's keys a package from it is checked against: the build's
/// ([`BUILD_STORE_KEY`] in `skills`) when it is there, else the factory's
/// one in `key_file` ([`STORE_KEY`]). Read again for every install, so a
/// round that writes or rotates the build's key needs no restart here.
///
/// A build's key file that is there but can't be read or is not one or two
/// keys refuses every install, said with the file's name; the factory's key
/// is not taken in its place. The build's file is there because the Store
/// moved off the factory's key, which may be the very key it rotated away
/// from (lost or leaked): falling back to it on a damaged file would quietly
/// trust that key again. Refusing costs only installs until the file is
/// put right (a build's own copy is rewritten by a round only while it is
/// still the build's, so a damaged one is mended by hand); a remove, the
/// app's off and on and every skill already installed go on as before.
pub fn store_keys(skills: &Path, key_file: &Path) -> Result<Vec<[u8; 32]>, String> {
    let built = skills.join(BUILD_STORE_KEY);
    match fs::read_to_string(&built) {
        Ok(text) => lamp_update::manifest::read_public_keys(&text).map_err(|why| {
            format!(
                "the Store's key ({}): {}; the factory's key is not taken in its place",
                built.display(),
                why.0
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            store_key(key_file).map(|key| vec![key])
        }
        Err(error) => Err(format!(
            "the Store's key ({}) can't be read: {error}; the factory's key is not taken in its place",
            built.display()
        )),
    }
}

/// The Store's public key from `key_file`, the factory's ([`STORE_KEY`]),
/// as `lamp-update` reads it.
pub fn store_key(key_file: &Path) -> Result<[u8; 32], String> {
    let text = fs::read_to_string(key_file).map_err(|error| {
        format!(
            "the Store's key ({}) can't be read: {error}",
            key_file.display()
        )
    })?;
    lamp_update::manifest::read_public_key(&text)
        .map_err(|why| format!("the Store's key ({}): {}", key_file.display(), why.0))
}

/// Installs the package at `url`, from the Skills Store, in `skills`, and
/// says what was done. Only as [`asked`] takes it, from root's manage
/// helper; `lamp-cloud` passes the URL and writes nothing.
///
/// In order, and nothing is written before the last step: the URL must be
/// https ([`store_url_ok`]); the package is fetched through `channel` into
/// memory, at most [`MOST_PACKAGE_BYTES`], within [`FETCH_WITHIN`]; its
/// bytes must have `sha256` when one is given; its signature must be the
/// Store's key's ([`store_keys`]) over its manifest, checked before
/// the manifest is read; its files, their paths (no `..`, no absolute path,
/// no hidden name, at most four parts), sizes, count and kinds (no link, no
/// device) as [`read_package_bytes`] checks them. Then it is installed as
/// any package is ([`install`]'s own steps, under the folder's lock, waited
/// for at most `lock_wait`, [`ASKED_LOCK_WAIT`] on the lamp, then [`BUSY`]
/// with nothing written): put
/// together where the running program does not look, loaded with every
/// check a load makes, refused when it is core, when a core skill has its
/// name or another skill its function, or when it asks the camera's
/// picture and the owner's yes to it is not kept in `state`'s
/// [`super::CAMERA_FILE`] ([`camera`]), and only then renamed into place.
pub fn install_from_url(
    skills: &Path,
    state: &Path,
    asked: &Asked<'_>,
    key_file: &Path,
    channel: &mut dyn Channel,
    lock_wait: Duration,
) -> Result<String, String> {
    if let Some(why) = asked.refused() {
        return Err(format!("not installed: {why}"));
    }
    let (url, sha256) = (asked.url, asked.sha256);
    if !fs::metadata(skills).is_ok_and(|meta| meta.is_dir()) {
        return Err(format!("no skills folder at {}", skills.display()));
    }
    let keys = store_keys(skills, key_file).map_err(|why| format!("not installed: {why}"))?;
    let mut bytes = Vec::new();
    channel
        .fetch(url, MOST_PACKAGE_BYTES, &mut bytes)
        .map_err(|why| format!("not installed: the package was not fetched: {why}"))?;
    if let Some(sha256) = sha256
        && sha256_hex(&bytes) != sha256
    {
        return Err(
            "not installed: the package's sha256 is not the one asked for: it was changed or damaged"
                .to_owned(),
        );
    }
    let mut package =
        read_package_bytes(&bytes, Some(&keys)).map_err(|why| format!("not installed: {why}"))?;
    drop(bytes);
    // What the catalogue's entry said, against what the channel signed.
    if let Some(name) = asked.name
        && name != package.manifest.name
    {
        return Err(format!(
            "not installed: the package is {}, not the {name} asked for",
            package.manifest.name
        ));
    }
    if let Some(version) = asked.version
        && version != package.manifest.version
    {
        return Err(format!(
            "not installed: the package is {} {}, not the version {version} asked for",
            package.manifest.name, package.manifest.version
        ));
    }
    // Waited for at most lock_wait, as a remove is: past it, BUSY with
    // nothing written.
    let _held = locked_within(skills, lock_wait)?;
    let swept = swept(skills);
    let files = std::mem::take(&mut package.files);
    let name = package.manifest.name.clone();
    put_in(
        skills,
        &swept,
        &name,
        &files,
        Some(&package),
        Some(state),
        true,
        &[],
    )
}

/// The Skills Store's side of the manage helper: where its key is, and how
/// a package is fetched (`lamp_update::fetch::Web` on a lamp, a local
/// server in the tests).
pub struct Store<'a> {
    pub key_file: PathBuf,
    pub channel: &'a mut dyn Channel,
}

/// Where the manage helper listens on a product lamp
/// (`deploy/product/lamp-skills-manage.socket`): root's socket, which only
/// the group `lampos` may connect to. systemd holds it and starts
/// `lamp-skills asked` as root for each connection, with the connection as
/// its standard input and output ([`asked`]).
pub const MANAGE_SOCKET: &str = "/run/lampos-skills-manage/manage.sock";

/// The units that may ask the manage helper, where the system's manager
/// puts them (each one's `Slice=` in its product unit): `lamp-cloud`, for
/// the app's remove and a Store install, and `lamp-voice`, for a made
/// skill's spoken yes (#1685). Any other part in the group `lampos` may
/// connect, but is refused before its line is read (the reviewer's P3 on
/// #2429), as the keys helper does (`lamp_setup::keys`, #2167).
pub const ASKERS: [&str; 3] = [
    APP_LINK,
    "/lampos.slice/lampos-voice.slice/lamp-voice.service",
    CONTROL,
];
/// `lamp-cloud`, the owner's app's link, where the system's manager puts
/// it: also the only peer whose `guard.on` and `guard.off` lamp-skills'
/// call socket takes (#2562, `watch::from_the_app`).
pub const APP_LINK: &str = "/lampos.slice/lampos-parts.slice/lamp-cloud.service";

/// The refusal to any other peer.
pub const NOT_AN_ASKER: &str =
    "only the app's link, the voice and lamp-control may change the lamp's skills";

/// Whether `/proc/<pid>/cgroup`'s text puts the process in one of
/// [`ASKERS`] (or a cgroup under one): the unified hierarchy's line
/// (`0::…`), or systemd's own on a hybrid one (`…:name=systemd:…`).
pub fn from_an_asker(cgroup: &str) -> bool {
    from_one_of(cgroup, &ASKERS)
}

/// Which of [`ASKERS`] `/proc/<pid>/cgroup`'s text puts the process in,
/// if any: lamp-control's own requests are not the app's.
pub fn asker_of(cgroup: &str) -> Option<Asker> {
    if from_one_of(cgroup, &[CONTROL]) {
        Some(Asker::Control)
    } else {
        from_one_of(cgroup, &ASKERS[..2]).then_some(Asker::App)
    }
}

/// Whether `/proc/<pid>/cgroup`'s text puts the process in one of `units`
/// (or a cgroup under one), as [`from_an_asker`] reads it.
pub fn from_one_of(cgroup: &str, units: &[&str]) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd")
            && units.iter().any(|asker| {
                path.strip_prefix(asker)
                    .is_some_and(|below| below.is_empty() || below.starts_with('/'))
            })
    })
}

/// In the helper's own environment (its unit's, never the peer's), `on`
/// lets any peer ask: only for the tests that drive `lamp-skills asked` over
/// a socket pair of their own, and those that drive the running program's
/// `guard` op on its call socket (#2562). No unit sets it, and
/// `lamp-update`'s product unit has no way to.
pub const ANY_ASKER: &str = "LAMP_SKILLS_ANY_ASKER";

/// Whether the peer on standard input (the socket systemd hands over) is
/// one of [`ASKERS`], by its credentials (`SO_PEERCRED`) and its cgroup.
/// Peer credentials are read on Linux only; elsewhere no peer is.
pub fn peer_is_an_asker() -> bool {
    peer_asker().is_some()
}

/// Which of [`ASKERS`] the peer on standard input is ([`asker_of`]), by
/// its credentials (`SO_PEERCRED`) and its cgroup; none elsewhere.
pub fn peer_asker() -> Option<Asker> {
    #[cfg(target_os = "linux")]
    {
        let stdin = std::io::stdin();
        rustix::net::sockopt::socket_peercred(&stdin)
            .ok()
            .and_then(|peer| {
                fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero())).ok()
            })
            .and_then(|cgroup| asker_of(&cgroup))
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The most bytes of a request to [`asked`].
pub const ASKED_MOST: u64 = 4_096;

/// What the manage helper says of a line that is no request.
pub const NOT_A_REQUEST: &str = "a request is one line, {\"remove\": NAME}, \
    {\"install\": {\"url\": URL, \"sha256\": HASH, \"name\": NAME, \"version\": VERSION}}, all but url optional, \
    {\"install_made\": SHA256}, {\"off\": NAME}, {\"on\": NAME}, {\"camera\": NAME} or {\"camera_off\": NAME}";

/// One request to the manage helper, read from `input`, done in `skills`,
/// and answered on `output` in one line: `{"done": WHAT}` or
/// `{"refused": WHY}`; true when it was done. Each is checked here, whoever
/// asked, not only by the asker (`lamp-cloud`'s `skills.remove` and
/// `skills.install`, #1722):
/// - `{"remove": NAME}`, a skill taken off by [`remove`]'s steps: a name
///   that is not a skill's and a core skill are refused; a skill renamed
///   aside whose old folder was not deleted is answered done, that
///   folder's path in the journal only;
/// - `{"install": {"url": URL, "sha256": HASH, "name": NAME, "version":
///   VERSION}}`, all but `url` optional (a catalogue entry's fields,
///   [`Asked`]), a package from the Skills Store installed by
///   [`install_from_url`]: only
///   over https, only signed with the Store's key, never core.
///   Without a `store`, refused;
/// - `{"install_made": SHA256}`, a skill made here by voice put on from the
///   voice's made-skills folder `made`, at its person's spoken yes
///   ([`install_made`]);
/// - `{"off": NAME}` or `{"on": NAME}`, the app's off and on (#2815), kept
///   in `state` (the running program's `--state-dir`) by [`turn`]: a skill
///   that is part of the lamp itself is refused with why;
/// - `{"camera": NAME}` or `{"camera_off": NAME}`, the owner's yes to the
///   skill seeing the camera's picture given or taken back in the app
///   (#3504's piece C), kept in `state` by [`camera`]; the Store installs
///   a package that asks the camera only once it is given.
///
/// Each waits at most `lock_wait` ([`ASKED_LOCK_WAIT`] on the lamp,
/// shorter only in a test) for the folder, then refuses with [`BUSY`]
/// having done nothing.
// Each place the helper may change is its own argument, as each comes
// from the unit's command line.
#[allow(clippy::too_many_arguments)]
pub fn asked(
    skills: &Path,
    made: &Path,
    state: &Path,
    lock_wait: Duration,
    store: Option<Store<'_>>,
    pushes: Option<Pushes<'_>>,
    input: impl Read,
    mut output: impl io::Write,
) -> io::Result<bool> {
    let mut line = String::new();
    // A request past its bound, or not UTF-8, is no request.
    let _ = io::BufReader::new(input.take(ASKED_MOST)).read_line(&mut line);
    let request: Option<serde_json::Value> = serde_json::from_str(line.trim_end()).ok();
    let only = |key: &str| {
        request
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .filter(|asked| asked.len() == 1)
            .and_then(|asked| asked.get(key))
    };
    let install = only("install").and_then(Asked::from_json);
    let pushed_asked = PUSHED_REQUESTS
        .iter()
        .find_map(|key| only(key).map(|value| (*key, value)));
    let done: Result<serde_json::Value, String> = match pushed_asked {
        // Skills pushed from a computer (#3869), asked by lamp-control.
        Some((key, value)) => match pushes {
            Some(pushes) if pushes.asker == Asker::App => Err(NOT_ITS_REQUEST.to_owned()),
            Some(pushes) => pushed_request(skills, state, pushes, key, value, lock_wait),
            None => Err(PUSH_OFF.to_owned()),
        },
        None if pushes.is_some_and(|pushes| pushes.asker == Asker::Control) => {
            Err(NOT_ITS_REQUEST.to_owned())
        }
        None => match (only("remove"), install) {
            _ if only("install_made").is_some() => match only("install_made") {
                Some(serde_json::Value::String(sha256)) => {
                    install_made(skills, Some(state), made, sha256, lock_wait)
                }
                _ => Err("a made skill is asked for as {\"install_made\": SHA256}".to_owned()),
            },
            // The app's off and on (#2815), kept where a `[stop]`'s are. A
            // pushed skill's are its own folder's: its on is also the
            // owner's yes to the picture, for that computer and those bytes.
            _ if only("off").or(only("on")).is_some() => {
                let pushed = |name: &str| {
                    pushes.filter(|pushes| pushed_from(&pushes.dev.join(name)).is_some())
                };
                match (only("off"), only("on")) {
                    (Some(serde_json::Value::String(name)), None) => match pushed(name) {
                        Some(pushes) => turn(pushes.dev, state, name, true, lock_wait),
                        None => turn(skills, state, name, true, lock_wait),
                    },
                    (None, Some(serde_json::Value::String(name))) => match pushed(name) {
                        Some(pushes) => said_yes(pushes.dev, name, lock_wait)
                            .and_then(|()| turn(pushes.dev, state, name, false, lock_wait)),
                        None => turn(skills, state, name, false, lock_wait),
                    },
                    _ => Err(NOT_A_REQUEST.to_owned()),
                }
            }
            // The owner's yes to the camera's picture, given or taken back.
            _ if only("camera").or(only("camera_off")).is_some() => {
                match (only("camera"), only("camera_off")) {
                    (Some(serde_json::Value::String(name)), None) => {
                        camera(skills, state, name, true, lock_wait)
                    }
                    (None, Some(serde_json::Value::String(name))) => {
                        camera(skills, state, name, false, lock_wait)
                    }
                    _ => Err(NOT_A_REQUEST.to_owned()),
                }
            }
            // Renamed aside is removed: where it was set aside, and why it
            // was not then deleted, go to the journal alone.
            (Some(serde_json::Value::String(name)), _) => {
                taken_off(skills, Some(state), name, Some(lock_wait)).map(|left| {
                    if let Some(left) = left {
                        eprintln!("lamp-skills: asked: removed {name}, but {left}");
                    }
                    removed_said(name)
                })
            }
            (_, Some(asked)) => match store {
                Some(store) => install_from_url(
                    skills,
                    state,
                    &asked,
                    &store.key_file,
                    store.channel,
                    lock_wait,
                ),
                None => Err(STORE_OFF.to_owned()),
            },
            _ => Err(NOT_A_REQUEST.to_owned()),
        }
        .map(serde_json::Value::from),
    };
    let answer = match &done {
        Ok(said) => serde_json::json!({ "done": said }),
        Err(why) if matches!(pushed_asked, Some(("install_pushed", _))) => {
            match refusal_code(why) {
                Some(code) => serde_json::json!({ "refused": why, "code": code }),
                None => serde_json::json!({ "refused": why }),
            }
        }
        Err(why) => serde_json::json!({ "refused": why }),
    };
    eprintln!("lamp-skills: asked: {answer}");
    writeln!(output, "{answer}")?;
    output.flush()?;
    Ok(done.is_ok())
}

/// The skills in `skills`, a line each: name, kind, version and what it
/// may use, or why it does not load, as the running program loads them
/// ([`load_each`]); then the functions they give the model.
pub fn list(skills: &Path) -> Result<String, String> {
    fs::read_dir(skills).map_err(|error| format!("{}: {error}", skills.display()))?;
    let mut loaded = Vec::new();
    let mut rows = vec![[
        "NAME".to_owned(),
        "KIND".to_owned(),
        "VERSION".to_owned(),
        "MAY USE".to_owned(),
    ]];
    for (folder, skill) in load_each(skills) {
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        rows.push(match skill {
            Ok(skill) => {
                let row = [
                    skill.name.clone(),
                    skill.kind.name().to_owned(),
                    skill.version.clone().unwrap_or_else(|| "-".into()),
                    if skill.enabled {
                        skill.permissions.said()
                    } else {
                        format!("disabled; {}", skill.permissions.said())
                    },
                ];
                loaded.push(skill);
                row
            }
            Err(why) => [
                name,
                kind_of(&folder).name().to_owned(),
                version_of(&folder).unwrap_or_else(|| "-".into()),
                format!("not loaded: {why}"),
            ],
        });
    }
    let widths: Vec<usize> = (0..3)
        .map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0))
        .collect();
    let mut listed: String = rows
        .iter()
        .map(|row| {
            format!(
                "{:<w0$}  {:<w1$}  {:<w2$}  {}\n",
                row[0],
                row[1],
                row[2],
                row[3],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2]
            )
        })
        .collect();
    listed.push_str(&functions_said(&loaded));
    listed.push('\n');
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reviewer's P3 on #2429: each asker's cgroup is where its product
    /// unit's `Slice=` puts it (systemd nests a dashed slice under each of
    /// its prefixes), so a moved slice fails here instead of refusing every
    /// request quietly.
    #[test]
    fn a_pushed_skills_time_is_said_in_utc() {
        assert_eq!(utc_text(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_text(1_793_341_365_361), "2026-10-30T06:22:45Z");
        assert_eq!(utc_text(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn the_askers_cgroups_are_their_product_units_slices() {
        for (unit, text) in [
            (
                "lamp-cloud.service",
                include_str!("../../../deploy/product/lamp-cloud.service"),
            ),
            (
                "lamp-voice.service",
                include_str!("../../../deploy/product/lamp-voice.service"),
            ),
        ] {
            let slice = text
                .lines()
                .find_map(|line| line.strip_prefix("Slice="))
                .expect("the unit names its slice")
                .trim();
            let name = slice.strip_suffix(".slice").unwrap();
            let mut path = String::new();
            for (at, _) in name.match_indices('-').chain([(name.len(), "")]) {
                path.push_str(&format!("/{}.slice", &name[..at]));
            }
            assert!(
                ASKERS.contains(&format!("{path}/{unit}").as_str()),
                "{path}/{unit}"
            );
        }
    }

    /// #2429: only lamp-cloud and the voice, at the system's place for each,
    /// may ask the manage helper; another part, or a same-named unit a user's
    /// manager made, may not.
    #[test]
    fn only_lamp_cloud_and_the_voice_may_ask_the_manage_helper() {
        assert!(from_an_asker(
            "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service\n"
        ));
        assert!(from_an_asker(
            "0::/lampos.slice/lampos-voice.slice/lamp-voice.service/worker\n"
        ));
        assert!(from_an_asker(
            "1:name=systemd:/lampos.slice/lampos-parts.slice/lamp-cloud.service\n"
        ));
        for other in [
            "0::/lampos.slice/lampos-parts.slice/lamp-body.service\n",
            "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service-not\n",
            "0::/user.slice/user-1000.slice/user@1000.service/lamp-cloud.service\n",
            "4:cpu:/lampos.slice/lampos-parts.slice/lamp-cloud.service\n",
            "",
        ] {
            assert!(!from_an_asker(other), "{other:?}");
        }
    }

    /// A file swapped after its folder was listed, for a link (to a file
    /// only root may read, run as root), another file or a fifo, is refused
    /// as it is opened and never read; the fifo at once, not waited on
    /// (the reviewer on #1698, 08:39).
    #[test]
    fn a_file_swapped_after_its_folder_was_listed_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        let secret = dir.path().join("secret");
        fs::write(&secret, b"root's alone").unwrap();
        fs::write(&path, b"a skill").unwrap();
        let listed = fs::symlink_metadata(&path).unwrap();
        let mut read = String::new();
        open_plain(&path, &listed)
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(read, "a skill");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&secret, &path).unwrap();
        let why = open_plain(&path, &listed).unwrap_err();
        assert_eq!(why, "a link: a skill is plain files and folders");
        // Another plain file in its place: the same name, not the same file.
        fs::remove_file(&path).unwrap();
        fs::hard_link(&secret, &path).unwrap();
        let why = open_plain(&path, &listed).unwrap_err();
        assert!(why.starts_with("changed while it was read"), "{why}");
        fs::remove_file(&path).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let why = open_plain(&path, &listed).unwrap_err();
        assert_eq!(why, "not a plain file");
    }

    /// Only what an install or a remove names its own folders is taken for
    /// one a killed run left.
    #[test]
    fn only_a_runs_own_folder_names_are_taken_for_what_it_left() {
        for name in [
            ".install-desk-tutor-4242",
            ".removed-desk-tutor-1",
            ".install-a-0",
        ] {
            assert!(left_by_a_run(name), "{name}");
        }
        for name in [
            "desk-tutor",
            "install-desk-tutor-4242",
            ".install-desk-tutor",
            ".install-desk-tutor-",
            ".install--4242",
            ".install-Desk-4242",
            ".install-desk_tutor-4242",
            ".install-desk-tutor-42a",
            ".install-desk-tutor-12345678901",
            ".installer-desk-tutor-4242",
            ".removed-../x-1",
            ".lamp-skills.lock",
        ] {
            assert!(!left_by_a_run(name), "{name}");
        }
    }

    /// Versions are ordered as numbers, part by part (`1.10.0` after
    /// `1.9.0`); the same is a reinstall; one that can't be ordered is
    /// refused.
    #[test]
    fn a_store_package_is_never_older_than_what_it_replaces() {
        for (installed, version) in [
            ("1.0.0", "1.0.0"),
            ("1.0.0", "1.0.1"),
            ("1.9.0", "1.10.0"),
            ("0.9.9", "1.0.0"),
        ] {
            assert_eq!(older_refused("tutor", Some(installed), version), None);
        }
        for (installed, version) in [
            ("1.0.1", "1.0.0"),
            ("1.10.0", "1.9.0"),
            ("2.0.0", "1.99.99"),
        ] {
            let why = older_refused("tutor", Some(installed), version).unwrap();
            assert!(why.contains("never put back to an older version"), "{why}");
        }
        for installed in [None, Some("1.0"), Some("v1.0.0"), Some("01.0.0")] {
            let why = older_refused("tutor", installed, "1.0.0").unwrap();
            assert!(why.contains("can't be compared"), "{why}");
        }
    }
}
