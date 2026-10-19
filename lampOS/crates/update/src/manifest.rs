//! What a build is, as its channel publishes it, and its signature.
//!
//! A build is a manifest: the channel, an increasing sequence, the commit,
//! where its files are, every binary with its SHA-256 and size, and the
//! units each lamp's own folder carries. The manifest's bytes are signed
//! with Ed25519 and published inside one small envelope, so the manifest
//! and its signature are always read together:
//!
//! ```json
//! {"format":"lampos-update/1","key":"<16 hex>","manifest":"<base64>","signature":"<base64>"}
//! ```
//!
//! The signature is over the exact manifest bytes, as V1's `autonomous-ota/v1`
//! envelope does, and the public key is the lamp's own, given at setup and
//! never read from the channel.

use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    digest,
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// The envelope's format: what this program reads.
pub const FORMAT: &str = "lampos-update/1";
/// The manifest's schema: what this program reads.
pub const SCHEMA: u32 = 1;
/// This updater's own level: raised whenever the updater learns to apply
/// something an older one cannot. A manifest whose `min_updater` is above
/// it is refused whole, before anything is fetched or written.
pub const UPDATER: u64 = 1;
/// The envelope's name in a channel's folder.
pub const ENVELOPE: &str = "lampos-manifest.json";
/// The largest envelope read: a manifest of every binary and every lamp's
/// units is a few kilobytes.
pub const MOST_ENVELOPE_BYTES: u64 = 512 * 1024;
/// The largest manifest.
pub const MOST_MANIFEST_BYTES: usize = 256 * 1024;
/// The largest file a build may carry. The largest binary today is about
/// 25 MB.
pub const MOST_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// The largest unit file.
pub const MOST_UNIT_BYTES: u64 = 64 * 1024;
/// At most this many binaries, and this many unit files, in one build.
pub const MOST_FILES: usize = 128;
pub const MOST_UNITS: usize = 512;

/// One binary of a build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// The binary's name, as it is installed: `lamp-voice`.
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

/// One unit of one lamp's own folder (`deploy/<lamp>/<unit>`), as the build
/// carries it. In the channel's folder it is `<lamp>.<unit>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub lamp: String,
    pub unit: String,
    pub sha256: String,
    pub size: u64,
}

impl Unit {
    /// Its name in the channel's folder.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.lamp, self.unit)
    }
}

/// A build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    /// The channel it was published on: `lamp-voice`.
    pub channel: String,
    /// Greater for every later build of the channel: CI's run number. A
    /// lamp never takes a build whose sequence is not greater than the one
    /// it runs, so an old signed manifest cannot be played back to it.
    pub sequence: u64,
    /// The commit it was built from: 40 hex characters.
    pub commit: String,
    /// The folder its files are in: `https://.../` or `file:///.../`.
    pub files_url: String,
    pub files: Vec<File>,
    pub units: Vec<Unit>,
    /// The oldest updater ([`UPDATER`]) that can apply this build. Left
    /// out, any can, and it is not written: the manifest is then byte for
    /// byte what an updater before the field reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_updater: Option<u64>,
}

/// The little of a manifest read before the rest, leniently: what a newer
/// manifest must keep readable to an older updater, so that it says why it
/// refuses and can find the updater the build carries.
#[derive(Deserialize)]
struct Needs {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    sequence: Option<u64>,
    #[serde(default)]
    min_updater: Option<u64>,
    #[serde(default)]
    files_url: Option<String>,
    #[serde(default)]
    files: Vec<serde_json::Value>,
}

/// A signed build that needs a newer updater than this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooOld {
    /// Its channel, when it is a plain name, and its sequence.
    pub channel: Option<String>,
    pub sequence: Option<u64>,
    /// Its `min_updater`.
    pub needed: u64,
    /// This updater's level.
    pub updater: u64,
    /// Where its files are, and the `lamp-update` it carries, if it names
    /// one plainly.
    pub files_url: Option<String>,
    pub carried: Option<File>,
}

impl TooOld {
    /// Why the build is refused, for a person.
    pub fn why(&self) -> String {
        let build = self
            .sequence
            .map_or_else(|| "the build".to_owned(), |n| format!("build {n}"));
        let wait = if self.carried.is_some() {
            "waiting for a newer updater"
        } else {
            "it carries no lamp-update, so waiting for a build that does"
        };
        format!(
            "{build} needs updater {} or newer, and this is updater {}; {wait}",
            self.needed, self.updater
        )
    }
}

/// The manifest bytes' need for a newer updater than `updater`, read before
/// anything else of them, so a build for a newer updater is refused for
/// that reason whatever else it carries.
pub fn too_old(manifest: &[u8], updater: u64) -> Option<TooOld> {
    let needs: Needs = serde_json::from_slice(manifest).ok()?;
    let needed = needs.min_updater?;
    if needed <= updater {
        return None;
    }
    let carried = needs
        .files
        .into_iter()
        .filter_map(|file| serde_json::from_value::<File>(file).ok())
        .find(|file| {
            file.name == "lamp-update"
                && check_hash_and_size(&file.name, &file.sha256, file.size, MOST_FILE_BYTES).is_ok()
        });
    let files_url = needs.files_url.filter(|url| is_folder_url(url));
    Some(TooOld {
        channel: needs.channel.filter(|channel| is_token(channel)),
        sequence: needs.sequence.filter(|sequence| *sequence > 0),
        needed,
        updater,
        carried: carried.filter(|_| files_url.is_some()),
        files_url,
    })
}

/// What is wrong with an envelope or a manifest. Said to a person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refused(why: impl Into<String>) -> Refused {
    Refused(why.into())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    /// The fingerprint of the key it was signed with, so that a manifest
    /// signed with another key is told apart from a damaged one.
    key: String,
    manifest: String,
    signature: String,
}

/// SHA-256 as 64 lowercase hex characters.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// A public key's fingerprint: the first 16 hex characters of its SHA-256,
/// as V1's OTA showed it, so that two people can say which key a lamp
/// trusts without passing the key around.
pub fn fingerprint(public_key: &[u8; 32]) -> String {
    sha256_hex(public_key)[..16].to_owned()
}

/// The lamp's public key, from its file: the 32 bytes in base64 on one
/// line, as V1's `signing_public_key`.
pub fn read_public_key(text: &str) -> Result<[u8; 32], Refused> {
    let bytes = STANDARD
        .decode(text.trim())
        .map_err(|_| refused("the public key is not base64"))?;
    bytes
        .try_into()
        .map_err(|_| refused("the public key is not 32 bytes"))
}

/// A private key, from its file: an unencrypted PKCS#8 Ed25519 key in
/// base64 (what `openssl genpkey -algorithm ed25519 -outform DER | base64`
/// prints, or ring's own version 2).
pub fn read_private_key(text: &str) -> Result<Ed25519KeyPair, Refused> {
    let compact: String = text.split_whitespace().collect();
    let bytes = STANDARD
        .decode(compact)
        .map_err(|_| refused("the private key is not base64"))?;
    Ed25519KeyPair::from_pkcs8_maybe_unchecked(&bytes)
        .map_err(|_| refused("the private key is not a PKCS#8 Ed25519 key"))
}

/// The public key of a private key, in base64: what a lamp is given.
pub fn public_key_text(key: &Ed25519KeyPair) -> String {
    STANDARD.encode(key.public_key().as_ref())
}

/// The envelope for these manifest bytes, signed with `key`. The bytes are
/// checked first, so nothing that a lamp would refuse is ever signed.
pub fn seal(manifest: &[u8], key: &Ed25519KeyPair) -> Result<Vec<u8>, Refused> {
    parse(manifest)?;
    let public: [u8; 32] = key
        .public_key()
        .as_ref()
        .try_into()
        .map_err(|_| refused("the key's public half is not 32 bytes"))?;
    let envelope = Envelope {
        format: FORMAT.to_owned(),
        key: fingerprint(&public),
        manifest: STANDARD.encode(manifest),
        signature: STANDARD.encode(key.sign(manifest).as_ref()),
    };
    let mut bytes = serde_json::to_vec(&envelope).map_err(|error| refused(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The manifest an envelope carries, if it was signed with `key` and is a
/// manifest this program can install from. The manifest's own bytes are
/// returned beside it: they are what is kept.
pub fn open(envelope: &[u8], key: &[u8; 32]) -> Result<(Manifest, Vec<u8>), Refused> {
    open_as(envelope, key, UPDATER)
}

/// [`open`], by an updater of level `updater`. Its `min_updater` is read
/// after the signature and before anything else, so a build for a newer
/// updater, whatever else it carries, is refused for that reason.
pub fn open_as(
    envelope: &[u8],
    key: &[u8; 32],
    updater: u64,
) -> Result<(Manifest, Vec<u8>), Refused> {
    match open_or_too_old(envelope, key, updater)? {
        Opened::Manifest(manifest, bytes) => Ok((manifest, bytes)),
        Opened::TooOld(too_old) => Err(refused(too_old.why())),
    }
}

/// A signed envelope, opened by an updater of level `updater`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// A manifest it can apply, and its bytes.
    Manifest(Manifest, Vec<u8>),
    /// Signed, and for a newer updater: what of it could be read.
    TooOld(TooOld),
}

/// [`open_as`], with a build for a newer updater told apart: a round may
/// still take the newer `lamp-update` it carries, and only that (#1680).
pub fn open_or_too_old(envelope: &[u8], key: &[u8; 32], updater: u64) -> Result<Opened, Refused> {
    if envelope.len() as u64 > MOST_ENVELOPE_BYTES {
        return Err(refused("the envelope is too large"));
    }
    let envelope: Envelope = serde_json::from_slice(envelope)
        .map_err(|error| refused(format!("the envelope cannot be read: {error}")))?;
    if envelope.format != FORMAT {
        return Err(refused(format!(
            "the envelope is {:?}, not {FORMAT}",
            envelope.format
        )));
    }
    let manifest = STANDARD
        .decode(&envelope.manifest)
        .map_err(|_| refused("the manifest is not base64"))?;
    let signature = STANDARD
        .decode(&envelope.signature)
        .map_err(|_| refused("the signature is not base64"))?;
    let ours = fingerprint(key);
    if envelope.key != ours {
        return Err(refused(format!(
            "signed with key {}, and this lamp trusts key {ours}",
            envelope.key
        )));
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&manifest, &signature)
        .map_err(|_| refused("the signature does not match the manifest"))?;
    if let Some(too_old) = too_old(&manifest, updater) {
        return Ok(Opened::TooOld(too_old));
    }
    let parsed = parse(&manifest)?;
    Ok(Opened::Manifest(parsed, manifest))
}

/// A manifest from its bytes, checked: every name plain, every hash a hash,
/// every size within bounds, nothing twice.
pub fn parse(bytes: &[u8]) -> Result<Manifest, Refused> {
    if bytes.len() > MOST_MANIFEST_BYTES {
        return Err(refused("the manifest is too large"));
    }
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| refused(format!("the manifest cannot be read: {error}")))?;
    check(&manifest)?;
    Ok(manifest)
}

fn check(manifest: &Manifest) -> Result<(), Refused> {
    if manifest.schema != SCHEMA {
        return Err(refused(format!(
            "the manifest is schema {}, and this program reads {SCHEMA}",
            manifest.schema
        )));
    }
    if !is_token(&manifest.channel) {
        return Err(refused("the channel's name is not plain"));
    }
    if manifest.sequence == 0 {
        return Err(refused("the sequence is 0"));
    }
    if manifest.min_updater == Some(0) {
        return Err(refused("min_updater is 0: leave it out instead"));
    }
    if manifest.commit.len() != 40 || !is_lower_hex(&manifest.commit) {
        return Err(refused("the commit is not 40 hex characters"));
    }
    if !is_folder_url(&manifest.files_url) {
        return Err(refused(
            "the files' folder is not an https:// or file:/// URL ending in /",
        ));
    }
    if manifest.files.is_empty() || manifest.files.len() > MOST_FILES {
        return Err(refused(format!(
            "a build has 1 to {MOST_FILES} files, and this one {}",
            manifest.files.len()
        )));
    }
    if manifest.units.len() > MOST_UNITS {
        return Err(refused(format!("more than {MOST_UNITS} units")));
    }
    let mut names = std::collections::BTreeSet::new();
    for file in &manifest.files {
        if !is_binary_name(&file.name) {
            return Err(refused(format!("{:?} is not a binary's name", file.name)));
        }
        if !names.insert(file.name.as_str()) {
            return Err(refused(format!("{} is listed twice", file.name)));
        }
        check_hash_and_size(&file.name, &file.sha256, file.size, MOST_FILE_BYTES)?;
    }
    let mut units = std::collections::BTreeSet::new();
    for unit in &manifest.units {
        if !is_token(&unit.lamp) || !is_unit_name(&unit.unit) {
            return Err(refused(format!(
                "{:?} of {:?} is not a unit of a lamp",
                unit.unit, unit.lamp
            )));
        }
        if !units.insert((unit.lamp.as_str(), unit.unit.as_str())) {
            return Err(refused(format!(
                "{} of {} is listed twice",
                unit.unit, unit.lamp
            )));
        }
        check_hash_and_size(&unit.file_name(), &unit.sha256, unit.size, MOST_UNIT_BYTES)?;
    }
    Ok(())
}

fn check_hash_and_size(name: &str, sha256: &str, size: u64, most: u64) -> Result<(), Refused> {
    if sha256.len() != 64 || !is_lower_hex(sha256) {
        return Err(refused(format!(
            "{name}'s SHA-256 is not 64 hex characters"
        )));
    }
    if size == 0 || size > most {
        return Err(refused(format!(
            "{name} is {size} bytes, and a file is 1 to {most}"
        )));
    }
    Ok(())
}

pub fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A plain name: a lamp (`lamp-4ace`), a channel (`lamp-voice`). Letters,
/// digits and `-`, beginning with a letter, at most 40.
pub fn is_token(text: &str) -> bool {
    (1..=40).contains(&text.len())
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A binary of lampOS: `lamp-` and a plain name. Never a path.
pub fn is_binary_name(text: &str) -> bool {
    text.strip_prefix("lamp-")
        .is_some_and(|rest| !rest.is_empty() && is_token(&format!("x{rest}")))
}

/// A part's unit: `lamp-voice.service`.
pub fn is_unit_name(text: &str) -> bool {
    text.strip_suffix(".service").is_some_and(is_binary_name)
}

fn is_folder_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("file:///"));
    rest.is_some_and(|rest| {
        url.ends_with('/')
            && !rest.contains("..")
            && !rest.contains(['?', '#', '\\', ' ', '@'])
            && rest.len() < 1024
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    /// A key made for this test and thrown away with it.
    pub(crate) fn throwaway() -> Ed25519KeyPair {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
    }

    fn public(key: &Ed25519KeyPair) -> [u8; 32] {
        key.public_key().as_ref().try_into().unwrap()
    }

    fn manifest() -> Manifest {
        Manifest {
            schema: 1,
            channel: "lamp-voice".into(),
            sequence: 145,
            commit: "bc3fccb546a22baaef180fcd57b3291a44e0e4c1".into(),
            files_url:
                "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-145/"
                    .into(),
            files: vec![File {
                name: "lamp-voice".into(),
                sha256: sha256_hex(b"voice"),
                size: 5,
            }],
            units: vec![Unit {
                lamp: "lamp-4ace".into(),
                unit: "lamp-voice.service".into(),
                sha256: sha256_hex(b"[Service]\n"),
                size: 10,
            }],
            min_updater: None,
        }
    }

    #[test]
    fn a_sealed_manifest_opens_with_its_key_and_gives_back_its_bytes() {
        let key = throwaway();
        let bytes = serde_json::to_vec(&manifest()).unwrap();
        let envelope = seal(&bytes, &key).unwrap();
        let (opened, kept) = open(&envelope, &public(&key)).unwrap();
        assert_eq!(opened, manifest());
        assert_eq!(kept, bytes);
    }

    #[test]
    fn another_key_a_changed_byte_or_a_changed_signature_is_refused() {
        let key = throwaway();
        let bytes = serde_json::to_vec(&manifest()).unwrap();
        let envelope = seal(&bytes, &key).unwrap();

        let other = throwaway();
        let why = open(&envelope, &public(&other)).unwrap_err().0;
        assert!(why.contains("signed with key"), "{why}");

        // The same manifest with one byte changed, under the right key's
        // fingerprint: the signature no longer matches.
        let mut parsed: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        let mut changed = bytes.clone();
        let at = changed.iter().position(|&byte| byte == b'5').unwrap();
        changed[at] = b'6';
        parsed["manifest"] = STANDARD.encode(&changed).into();
        let why = open(&serde_json::to_vec(&parsed).unwrap(), &public(&key))
            .unwrap_err()
            .0;
        assert_eq!(why, "the signature does not match the manifest");

        // The other key's signature, with this key's fingerprint written in.
        let forged = seal(&bytes, &other).unwrap();
        let mut forged: serde_json::Value = serde_json::from_slice(&forged).unwrap();
        forged["key"] = fingerprint(&public(&key)).into();
        let why = open(&serde_json::to_vec(&forged).unwrap(), &public(&key))
            .unwrap_err()
            .0;
        assert_eq!(why, "the signature does not match the manifest");
    }

    #[test]
    fn a_key_made_by_openssl_signs_as_well() {
        // `openssl genpkey -algorithm ed25519 -outform DER` writes PKCS#8
        // version 1: this fixed prefix, then the 32-byte seed.
        let seed = [7u8; 32];
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(&seed);
        let key = read_private_key(&STANDARD.encode(&der)).unwrap();
        let from_seed = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        assert_eq!(key.public_key().as_ref(), from_seed.public_key().as_ref());
        let lamp = read_public_key(&public_key_text(&key)).unwrap();
        let envelope = seal(&serde_json::to_vec(&manifest()).unwrap(), &key).unwrap();
        assert!(open(&envelope, &lamp).is_ok());
    }

    /// One way of making a manifest wrong.
    type Wrong = Box<dyn Fn(&mut Manifest)>;

    #[test]
    fn nothing_but_a_plain_manifest_is_signed_or_taken() {
        let key = throwaway();
        let cases: Vec<(&str, Wrong)> = vec![
            ("schema", Box::new(|m| m.schema = 2)),
            ("sequence", Box::new(|m| m.sequence = 0)),
            ("min_updater", Box::new(|m| m.min_updater = Some(0))),
            ("commit", Box::new(|m| m.commit = "main".into())),
            (
                "folder",
                Box::new(|m| m.files_url = "http://example.com/".into()),
            ),
            (
                "folder",
                Box::new(|m| m.files_url = "https://example.com/x".into()),
            ),
            (
                "folder",
                Box::new(|m| m.files_url = "https://example.com/../".into()),
            ),
            (
                "path",
                Box::new(|m| m.files[0].name = "../lamp-voice".into()),
            ),
            (
                "path",
                Box::new(|m| m.files[0].name = "lamp-voice/x".into()),
            ),
            ("not lamp", Box::new(|m| m.files[0].name = "bash".into())),
            ("hash", Box::new(|m| m.files[0].sha256 = "ABC".into())),
            ("size", Box::new(|m| m.files[0].size = MOST_FILE_BYTES + 1)),
            ("twice", Box::new(|m| m.files.push(m.files[0].clone()))),
            (
                "unit",
                Box::new(|m| m.units[0].unit = "lamp-voice.timer".into()),
            ),
            ("lamp", Box::new(|m| m.units[0].lamp = "../etc".into())),
        ];
        for (what, change) in cases {
            let mut wrong = manifest();
            change(&mut wrong);
            let bytes = serde_json::to_vec(&wrong).unwrap();
            assert!(seal(&bytes, &key).is_err(), "{what} was sealed");
            assert!(parse(&bytes).is_err(), "{what} was taken");
        }
        // A field this version does not know is refused, not passed over:
        // an installer does not guess what a newer manifest meant.
        let mut value = serde_json::to_value(manifest()).unwrap();
        value["requires"] = "something".into();
        assert!(parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn a_build_for_a_newer_updater_is_refused_by_an_older_one_only() {
        let key = throwaway();
        let mut needing = manifest();
        needing.min_updater = Some(3);
        let bytes = serde_json::to_vec(&needing).unwrap();
        let envelope = seal(&bytes, &key).unwrap();
        let why = open_as(&envelope, &public(&key), 2).unwrap_err().0;
        assert_eq!(
            why,
            "build 145 needs updater 3 or newer, and this is updater 2; \
             it carries no lamp-update, so waiting for a build that does"
        );
        // One that carries an updater says so, and names it.
        let mut carrying = needing.clone();
        let updater = File {
            name: "lamp-update".into(),
            sha256: sha256_hex(b"update 3"),
            size: 8,
        };
        carrying.files.push(updater.clone());
        let bytes_carrying = serde_json::to_vec(&carrying).unwrap();
        let found = too_old(&bytes_carrying, 2).unwrap();
        assert_eq!(found.carried, Some(updater));
        assert_eq!(
            found.files_url.as_deref(),
            Some(carrying.files_url.as_str())
        );
        assert!(
            found.why().ends_with("; waiting for a newer updater"),
            "{}",
            found.why()
        );
        assert_eq!(too_old(&bytes_carrying, 3), None);
        // An updater as new or newer takes it, as it is.
        for updater in [3, 4] {
            let (opened, kept) = open_as(&envelope, &public(&key), updater).unwrap();
            assert_eq!(opened, needing);
            assert_eq!(kept, bytes);
        }
        // Without the field, any updater takes it.
        let envelope = seal(&serde_json::to_vec(&manifest()).unwrap(), &key).unwrap();
        assert!(open_as(&envelope, &public(&key), 1).is_ok());
        assert!(open(&envelope, &public(&key)).is_ok());
    }

    #[test]
    fn a_newer_manifest_this_updater_cannot_read_is_refused_for_its_updater() {
        // What a later schema may carry: refused for the updater it needs,
        // not for its schema or its new fields. Signed by hand, since seal
        // checks it is a manifest this program reads.
        let key = throwaway();
        let mut value = serde_json::to_value(manifest()).unwrap();
        value["schema"] = 2.into();
        value["product_units"] = serde_json::json!([]);
        value["min_updater"] = (UPDATER + 1).into();
        let bytes = serde_json::to_vec(&value).unwrap();
        let envelope = serde_json::json!({
            "format": FORMAT,
            "key": fingerprint(&public(&key)),
            "manifest": STANDARD.encode(&bytes),
            "signature": STANDARD.encode(key.sign(&bytes).as_ref()),
        });
        let why = open(&serde_json::to_vec(&envelope).unwrap(), &public(&key))
            .unwrap_err()
            .0;
        assert!(
            why.contains(&format!("needs updater {}", UPDATER + 1)),
            "{why}"
        );
    }

    #[test]
    fn min_updater_is_signed_with_the_rest() {
        let key = throwaway();
        let mut needing = manifest();
        needing.min_updater = Some(UPDATER + 1);
        let bytes = serde_json::to_vec(&needing).unwrap();
        let envelope = seal(&bytes, &key).unwrap();
        // Lowered on the way, under the same signature: refused, and not
        // as a build this updater could apply.
        let lowered = String::from_utf8(bytes)
            .unwrap()
            .replace(
                &format!("\"min_updater\":{}", UPDATER + 1),
                &format!("\"min_updater\":{UPDATER}"),
            )
            .into_bytes();
        let mut parsed: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        parsed["manifest"] = STANDARD.encode(&lowered).into();
        let why = open(&serde_json::to_vec(&parsed).unwrap(), &public(&key))
            .unwrap_err()
            .0;
        assert_eq!(why, "the signature does not match the manifest");
        // Taken out, likewise.
        let mut value: serde_json::Value = serde_json::from_slice(&lowered).unwrap();
        value.as_object_mut().unwrap().remove("min_updater");
        parsed["manifest"] = STANDARD.encode(serde_json::to_vec(&value).unwrap()).into();
        let why = open(&serde_json::to_vec(&parsed).unwrap(), &public(&key))
            .unwrap_err()
            .0;
        assert_eq!(why, "the signature does not match the manifest");
    }

    #[test]
    fn min_updater_raised_under_the_same_signature_is_a_bad_signature() {
        // The case that matters: a build signed for this updater, then
        // marked for a newer one on the way. The field is read only after
        // the signature, so it is refused as tampered, not climbed.
        let key = throwaway();
        let signed_without = serde_json::to_vec(&manifest()).unwrap();
        let mut at_this = manifest();
        at_this.min_updater = Some(UPDATER);
        let signed_at_this = serde_json::to_vec(&at_this).unwrap();
        for signed in [signed_without, signed_at_this] {
            let envelope = seal(&signed, &key).unwrap();
            let mut raised: serde_json::Value = serde_json::from_slice(&signed).unwrap();
            raised["min_updater"] = (UPDATER + 1).into();
            let raised = serde_json::to_vec(&raised).unwrap();
            assert!(too_old(&raised, UPDATER).is_some());
            let mut parsed: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
            parsed["manifest"] = STANDARD.encode(&raised).into();
            let tampered = serde_json::to_vec(&parsed).unwrap();
            assert_eq!(
                open_or_too_old(&tampered, &public(&key), UPDATER)
                    .unwrap_err()
                    .0,
                "the signature does not match the manifest"
            );
            assert_eq!(
                open(&tampered, &public(&key)).unwrap_err().0,
                "the signature does not match the manifest"
            );
        }
    }

    #[test]
    fn a_manifest_without_min_updater_is_written_and_read_as_before() {
        let bytes = serde_json::to_vec(&manifest()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("min_updater"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "channel",
                "commit",
                "files",
                "files_url",
                "schema",
                "sequence",
                "units"
            ]
        );
        assert_eq!(parse(&bytes).unwrap().min_updater, None);
    }

    #[test]
    fn names_are_plain() {
        assert!(is_binary_name("lamp-voice"));
        assert!(is_binary_name("lamp-turn-end"));
        assert!(!is_binary_name("lamp-"));
        assert!(!is_binary_name("lamp-Voice"));
        assert!(!is_binary_name("lamp-voice.d"));
        assert!(is_unit_name("lamp-voice.service"));
        assert!(!is_unit_name("lampos.target"));
        assert!(is_token("lamp-4ace"));
        assert!(!is_token("4ace"));
        assert_eq!(fingerprint(&[0; 32]).len(), 16);
    }
}
