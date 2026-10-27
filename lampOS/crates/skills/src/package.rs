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
/// deleted first ([`swept`]).
pub fn install(skills: &Path, from: &Path) -> Result<String, String> {
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
    put_in(skills, &swept, &name, &files, package.as_ref())
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
    made: &Path,
    sha256: &str,
    wait: Duration,
) -> Result<String, String> {
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    install_made_at(skills, made, sha256, wait, now_unix_ms)
}

/// [`install_made`] at `now_unix_ms`: only the made skill the lamp asked
/// about last, within `lamp_voice::made_skill::PUT_ON_WITHIN` and not yet
/// on, as the voice's own `put_on` (the first builder on #2602, 22 Oct
/// 10:1x). A kept SHA-256 asked for later, or one not asked about at all,
/// puts nothing on, whoever asks. The time is the helper's, never the
/// asker's.
pub fn install_made_at(
    skills: &Path,
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
    put_in(skills, &swept, &name, &files, Some(&package)).map(|said| {
        said.replace(
            NO_SIGNATURE,
            &format!(
                "bound to the person's yes by its SHA-256, {}",
                &sha256[..12]
            ),
        )
    })
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
/// the caller and what a killed run left `swept`.
fn put_in(
    skills: &Path,
    swept: &[String],
    name: &str,
    files: &Files,
    package: Option<&Package>,
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
        open_to_parts(&folder).map_err(failed)?;
        moved_in(skills, &name, &folder, files.len(), package)
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
/// skill is refused: it comes with the lamp.
pub fn remove(skills: &Path, name: &str) -> Result<String, String> {
    match taken_off(skills, name, None)? {
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
/// on are kept ([`super::OFF_FILE`], [`super::ON_FILE`]).
pub const STATE_DIR: &str = "/var/lib/lampos-skills-state";

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
fn taken_off(skills: &Path, name: &str, wait: Option<Duration>) -> Result<Option<String>, String> {
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
/// name or another skill its function, and only then renamed into place.
pub fn install_from_url(
    skills: &Path,
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
    put_in(skills, &swept, &name, &files, Some(&package))
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
pub const ASKERS: [&str; 2] = [
    APP_LINK,
    "/lampos.slice/lampos-voice.slice/lamp-voice.service",
];
/// `lamp-cloud`, the owner's app's link, where the system's manager puts
/// it: also the only peer whose `guard.on` and `guard.off` lamp-skills'
/// call socket takes (#2562, `watch::from_the_app`).
pub const APP_LINK: &str = "/lampos.slice/lampos-parts.slice/lamp-cloud.service";

/// The refusal to any other peer.
pub const NOT_AN_ASKER: &str = "only the app's link and the voice may change the lamp's skills";

/// Whether `/proc/<pid>/cgroup`'s text puts the process in one of
/// [`ASKERS`] (or a cgroup under one): the unified hierarchy's line
/// (`0::…`), or systemd's own on a hybrid one (`…:name=systemd:…`).
pub fn from_an_asker(cgroup: &str) -> bool {
    from_one_of(cgroup, &ASKERS)
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
    #[cfg(target_os = "linux")]
    {
        let stdin = std::io::stdin();
        rustix::net::sockopt::socket_peercred(&stdin).is_ok_and(|peer| {
            fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero()))
                .is_ok_and(|cgroup| from_an_asker(&cgroup))
        })
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// The most bytes of a request to [`asked`].
pub const ASKED_MOST: u64 = 4_096;

/// What the manage helper says of a line that is no request.
pub const NOT_A_REQUEST: &str = "a request is one line, {\"remove\": NAME}, \
    {\"install\": {\"url\": URL, \"sha256\": HASH, \"name\": NAME, \"version\": VERSION}}, all but url optional, \
    {\"install_made\": SHA256}, {\"off\": NAME} or {\"on\": NAME}";

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
///   that is part of the lamp itself is refused with why.
///
/// Each waits at most `lock_wait` ([`ASKED_LOCK_WAIT`] on the lamp,
/// shorter only in a test) for the folder, then refuses with [`BUSY`]
/// having done nothing.
pub fn asked(
    skills: &Path,
    made: &Path,
    state: &Path,
    lock_wait: Duration,
    store: Option<Store<'_>>,
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
    let done = match (only("remove"), install) {
        _ if only("install_made").is_some() => match only("install_made") {
            Some(serde_json::Value::String(sha256)) => {
                install_made(skills, made, sha256, lock_wait)
            }
            _ => Err("a made skill is asked for as {\"install_made\": SHA256}".to_owned()),
        },
        // The app's off and on (#2815), kept where a `[stop]`'s are.
        _ if only("off").or(only("on")).is_some() => match (only("off"), only("on")) {
            (Some(serde_json::Value::String(name)), None) => {
                turn(skills, state, name, true, lock_wait)
            }
            (None, Some(serde_json::Value::String(name))) => {
                turn(skills, state, name, false, lock_wait)
            }
            _ => Err(NOT_A_REQUEST.to_owned()),
        },
        // Renamed aside is removed: where it was set aside, and why it
        // was not then deleted, go to the journal alone.
        (Some(serde_json::Value::String(name)), _) => {
            taken_off(skills, name, Some(lock_wait)).map(|left| {
                if let Some(left) = left {
                    eprintln!("lamp-skills: asked: removed {name}, but {left}");
                }
                removed_said(name)
            })
        }
        (_, Some(asked)) => match store {
            Some(store) => {
                install_from_url(skills, &asked, &store.key_file, store.channel, lock_wait)
            }
            None => Err(STORE_OFF.to_owned()),
        },
        _ => Err(NOT_A_REQUEST.to_owned()),
    };
    let answer = match &done {
        Ok(said) => serde_json::json!({ "done": said }),
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
