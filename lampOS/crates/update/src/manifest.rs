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
/// it is refused whole, before anything is fetched or written. 2: the
/// build's `models` (#1680). 3: its `settings` (#1680). 4: the image's
/// own files, `image` (#1369). 5: the reset plan, `reset_plan` (#806).
/// 6: ordinary product service units carried under [`PRODUCT_LAMP`].
/// 7: a part's templates, sockets and timers under it too
/// (crate::companion). 8: a product unit the lamp lacks added
/// (crate::added).
pub const UPDATER: u64 = 8;
/// Reserved lamp marker for the product's ordinary service templates.
pub const PRODUCT_LAMP: &str = "product";
/// Older updaters do not select product-marked units.
pub const PRODUCT_UNITS_UPDATER: u64 = 6;
/// The level a build that carries a part's templates, sockets or timers
/// needs: an older updater refuses their names, so it climbs first.
pub const COMPANIONS_UPDATER: u64 = 7;
/// The level a build needs for a round to add the product units a lamp
/// lacks: an older updater takes only the units a lamp has, so it climbs
/// first. A round adds only from a build that asks for this level.
pub const ADDS_UPDATER: u64 = 8;
/// The level a build that carries models needs: an updater below it does
/// not read the field.
pub const MODELS_UPDATER: u64 = 2;
/// The level a build that carries settings needs.
pub const SETTINGS_UPDATER: u64 = 3;
/// The level a build that carries the image's files needs.
pub const IMAGE_UPDATER: u64 = 4;
/// The level a build that carries the reset plan needs.
pub const RESET_PLAN_UPDATER: u64 = 5;
/// Setup's reset plan (#806): what the 5 s reset wipes. The product's and
/// never the person's, so a build carries it in a field of its own
/// (`reset_plan`), never in `settings`, and it is written over any plan a
/// lamp has (crate::apply, docs/updates.md).
pub const RESET_PLAN: &str = "/etc/lampOS/setup/reset-plan.json";
/// The image's own files a build may carry (#1369): those of
/// `robots/lamp/rootfs/` and its overrides that are the product's to keep
/// up to date after a lamp ships. Not V1's HAL (`/opt/hal`, the LED boot
/// units, `hal.service`'s drop-in), and never a drop-in a lamp or a person
/// made: only these paths.
pub const IMAGE_FILES: &[&str] = &[
    "/etc/asound.conf",
    "/etc/default/orangepi-ramlog",
    "/etc/systemd/system/lamp-cmedia-speaker.service",
    "/etc/systemd/system/respeaker-lite-softvol.service",
    "/etc/systemd/system/logrotate.service.d/zz-lamp-ramlog.conf",
    "/etc/systemd/system/orangepi-ramlog.service.d/zz-lamp-journal-keep.conf",
    "/etc/systemd/journald.conf.d/zz-lamp-journal.conf",
    "/etc/udev/rules.d/90-respeaker-lite.rules",
    "/etc/udev/rules.d/91-pulseaudio-hal-ignore.rules",
    "/etc/udev/rules.d/99-lamp-device.rules",
    "/usr/local/libexec/lamp-journal-keep",
    "/usr/local/libexec/lamp-ramlog-postrotate",
];
/// At most this many of the image's files in one build, all variants.
pub const MOST_IMAGE: usize = 64;
/// At most this many SHA-256s the image has shipped, for one file.
pub const MOST_SHIPPED: usize = 64;
/// At most this many settings in one build, and this many bytes of them in
/// all: today's are 14 files and about 40 KB.
pub const MOST_SETTINGS: usize = 128;
pub const MOST_SETTINGS_BYTES: usize = 160 * 1024;
/// The settings a build may carry: these files, and any file under these
/// folders. Never a secret key, setup's answers, the lamp's own poses or
/// the update key: those are the person's or the lamp's (docs/updates.md).
/// The one key a build carries is the Skills Store's public key in the
/// skills folder (`install::STORE_KEY_SETTING`, #3692).
pub const SETTING_FILES: &[&str] = &["/etc/lampOS/records.json", "/etc/tmpfiles.d/lampos.conf"];
pub const SETTING_FOLDERS: &[&str] = &[
    "/etc/lampOS/expression/",
    "/etc/lampOS/controls/",
    "/var/lib/lampOS/skills/",
];
/// The modes a setting may have.
pub const SETTING_MODES: &[&str] = &["0644", "0600"];
/// At most this many models in one build.
pub const MOST_MODELS: usize = 64;
/// Where models live: their paths are under it, and its `models/` is the
/// store, each model named by its SHA-256.
pub const MODELS_ROOT: &str = "/opt/lampOS/";
pub const STORE: &str = "/opt/lampOS/models/";
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

/// One unit of a lamp's own folder, or an ordinary product service under
/// [`PRODUCT_LAMP`]. In the channel's folder it is `<lamp>.<unit>`.
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

    /// The unit as systemd names it on a lamp ([`lamp_name`]).
    pub fn on_lamp(&self) -> String {
        lamp_name(&self.unit)
    }
}

/// How a manifest names a part's template: `lamp-setup-wifi.template.service`
/// for `lamp-setup-wifi@.service`. Its file in the channel is `<lamp>.<unit>`,
/// a release asset, whose name can't hold `@`. No part's service has a `.`
/// in its name, so the two forms never meet.
const TEMPLATE: &str = ".template.service";

/// A unit's name on a lamp, from its name in a manifest: a template's `@`
/// given back, any other name as it is.
pub fn lamp_name(unit: &str) -> String {
    match unit.strip_suffix(TEMPLATE) {
        Some(stem) => format!("{stem}@.service"),
        None => unit.to_owned(),
    }
}

/// A unit's name in a manifest, from its name on a lamp ([`lamp_name`]).
pub fn manifest_name(unit: &str) -> String {
    match unit.strip_suffix("@.service") {
        Some(stem) => format!("{stem}{TEMPLATE}"),
        None => unit.to_owned(),
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
    /// The models the build's parts read, which no binary carries (#1680):
    /// fetched from their own URL, checked, and switched with the parts.
    /// Left out when there are none, and not written: the manifest is then
    /// byte for byte what an updater before the field reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<Model>,
    /// The product's settings and tmpfiles, inline (#1680): written by
    /// `lamp-update-apply` at the switch, each only where the lamp has not
    /// changed it. Left out when there are none, and not written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
    /// The image's own files (#1369), for each variant of the lamp: written
    /// by `lamp-update-apply` only where the lamp has one the image shipped.
    /// Not in `settings`, which a `lamp-update-back` older than the field
    /// would refuse whole: one that old ignores this field. Left out when
    /// there are none, and not written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image: Vec<ImageFile>,
    /// Setup's reset plan at [`RESET_PLAN`] (#806): written by
    /// `lamp-update-apply` over the lamp's plan wherever there is one, the
    /// old one kept in the records first. Not in `settings`, for the same
    /// reason as `image`. Left out when there is none, and not written.
    /// Boxed, so a manifest stays small beside [`TooOld`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_plan: Option<Box<Setting>>,
}

/// One of the image's own files, for one variant of the lamp
/// (`/etc/autonomous/hardware-profile`, none being `standard`). Read by
/// `lamp-update-apply`, so it is a contract for good, as [`Setting`] is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageFile {
    pub variant: String,
    pub path: String,
    pub text: String,
    pub sha256: String,
    /// Every SHA-256 the image has shipped at this path for this variant:
    /// a lamp's file that is one of them is the image's, never the lamp's
    /// own, since install never recorded these.
    #[serde(default)]
    pub shipped: Vec<String>,
}

impl ImageFile {
    /// As a setting: what the three-way rule reads.
    pub fn setting(&self) -> Setting {
        Setting {
            path: self.path.clone(),
            text: Some(self.text.clone()),
            sha256: Some(self.sha256.clone()),
            mode: Some("0644".to_owned()),
            removed: false,
        }
    }
}

/// A variant's name: what `apply-overrides.py` reads, plain.
pub fn is_variant(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The image's files checked: a path of [`IMAGE_FILES`] and a variant, once
/// each pair; the text's SHA-256; every shipped SHA-256 a hash.
pub fn check_image(image: &[ImageFile]) -> Result<(), Refused> {
    if image.len() > MOST_IMAGE {
        return Err(refused(format!(
            "more than {MOST_IMAGE} of the image's files"
        )));
    }
    let mut pairs = std::collections::BTreeSet::new();
    let mut bytes = 0;
    for file in image {
        if !IMAGE_FILES.contains(&file.path.as_str()) {
            return Err(refused(format!(
                "{:?} is not one of the image's files a build may carry",
                file.path
            )));
        }
        if !is_variant(&file.variant) {
            return Err(refused(format!(
                "{:?} is not a variant's name",
                file.variant
            )));
        }
        if !pairs.insert((file.variant.as_str(), file.path.as_str())) {
            return Err(refused(format!(
                "{} of {} is listed twice",
                file.path, file.variant
            )));
        }
        if sha256_hex(file.text.as_bytes()) != file.sha256 {
            return Err(refused(format!(
                "{} of {}: its text is not its SHA-256",
                file.path, file.variant
            )));
        }
        if file.shipped.len() > MOST_SHIPPED
            || file
                .shipped
                .iter()
                .any(|sha| sha.len() != 64 || !is_lower_hex(sha))
        {
            return Err(refused(format!(
                "{} of {}: its shipped SHA-256s are not up to {MOST_SHIPPED} hashes",
                file.path, file.variant
            )));
        }
        bytes += file.text.len();
    }
    if bytes > MOST_SETTINGS_BYTES {
        return Err(refused(format!(
            "the image's files are more than {MOST_SETTINGS_BYTES} bytes"
        )));
    }
    Ok(())
}

/// A setting a build carries: its path, and its text, SHA-256 and mode; or
/// `removed`, without them. Read by `lamp-update-apply`, a copy only
/// `install` writes, so it is a contract for good: these fields keep their
/// meaning, and one it does not know is ignored, never refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// `0644` or `0600`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The build no longer has it: removed where it is still the build's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

impl Setting {
    /// A setting with this text, 0644.
    pub fn new(path: &str, text: &str) -> Self {
        Self {
            path: path.to_owned(),
            text: Some(text.to_owned()),
            sha256: Some(sha256_hex(text.as_bytes())),
            mode: Some("0644".to_owned()),
            removed: false,
        }
    }

    /// Its mode as a number.
    pub fn mode_bits(&self) -> u32 {
        self.mode
            .as_deref()
            .and_then(|mode| u32::from_str_radix(mode, 8).ok())
            .unwrap_or(0o644)
    }
}

/// A setting's path: one of [`SETTING_FILES`], or a file under one of
/// [`SETTING_FOLDERS`] whose parts are plain.
pub fn is_setting_path(path: &str) -> bool {
    if SETTING_FILES.contains(&path) {
        return true;
    }
    path.len() < 512
        && !path.ends_with('/')
        && SETTING_FOLDERS.iter().any(|folder| {
            path.strip_prefix(folder).is_some_and(|rest| {
                rest.split('/').all(|part| {
                    !part.is_empty()
                        && !part.starts_with('.')
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                })
            })
        })
}

/// Each setting checked: a path it may have, once; its text with its
/// SHA-256 and a mode it may have, or `removed` with neither.
pub fn check_settings(settings: &[Setting]) -> Result<(), Refused> {
    if settings.len() > MOST_SETTINGS {
        return Err(refused(format!("more than {MOST_SETTINGS} settings")));
    }
    let mut paths = std::collections::BTreeSet::new();
    let mut bytes = 0;
    for setting in settings {
        if !is_setting_path(&setting.path) {
            return Err(refused(format!(
                "{:?} is not a path a build's settings may have",
                setting.path
            )));
        }
        if !paths.insert(setting.path.as_str()) {
            return Err(refused(format!("{} is listed twice", setting.path)));
        }
        bytes += check_setting(setting)?;
    }
    if bytes > MOST_SETTINGS_BYTES {
        return Err(refused(format!(
            "the settings are {bytes} bytes, and at most {MOST_SETTINGS_BYTES}"
        )));
    }
    Ok(())
}

/// The reset plan checked: at [`RESET_PLAN`], never removed, its text with
/// its SHA-256, 0644 (every part's reset reads it), and at most 16 KiB, as
/// `lamp-setup` reads.
pub fn check_reset_plan(plan: &Setting) -> Result<(), Refused> {
    if plan.path != RESET_PLAN {
        return Err(refused(format!(
            "{:?} is not the reset plan's path, {RESET_PLAN}",
            plan.path
        )));
    }
    if plan.removed {
        return Err(refused("the reset plan is never removed by a build"));
    }
    if plan.mode.as_deref() != Some("0644") {
        return Err(refused("the reset plan's mode is not 0644"));
    }
    if check_setting(plan)? > 16 * 1024 {
        return Err(refused("the reset plan exceeds 16 KiB"));
    }
    Ok(())
}

/// One setting's text, SHA-256 and mode checked: its text's length, or 0
/// for one removed.
fn check_setting(setting: &Setting) -> Result<usize, Refused> {
    if setting.removed {
        if setting.text.is_some() || setting.sha256.is_some() || setting.mode.is_some() {
            return Err(refused(format!(
                "{} is removed, and has a text, SHA-256 or mode",
                setting.path
            )));
        }
        return Ok(0);
    }
    let (Some(text), Some(sha256)) = (&setting.text, &setting.sha256) else {
        return Err(refused(format!("{} has no text or SHA-256", setting.path)));
    };
    if sha256_hex(text.as_bytes()) != *sha256 {
        return Err(refused(format!(
            "{} does not match its SHA-256",
            setting.path
        )));
    }
    if !setting
        .mode
        .as_deref()
        .is_some_and(|mode| SETTING_MODES.contains(&mode))
    {
        return Err(refused(format!(
            "{}'s mode is not one of {}",
            setting.path,
            SETTING_MODES.join(", ")
        )));
    }
    Ok(text.len())
}

/// A model a part reads: where it goes on the lamp, where it is fetched
/// from, and its size and SHA-256 (`deploy/product/models.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub path: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
    /// The file is one member of a gzipped tar at `url`, itself pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<Archive>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub bytes: u64,
    pub sha256: String,
    pub member: String,
}

impl Model {
    /// What fetching it costs on the disk: the model, and its archive
    /// beside it while it is taken out.
    pub fn fetch_bytes(&self) -> u64 {
        self.bytes
            .saturating_add(self.archive.as_ref().map_or(0, |archive| archive.bytes))
    }
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

/// The most keys a key file of several holds: a key's new and old during a
/// rotation.
pub const MOST_PUBLIC_KEYS: usize = 2;

/// Up to [`MOST_PUBLIC_KEYS`] public keys, one a line, each exactly as the
/// factory writes one (`^[A-Za-z0-9+/]{43}=$`, 32 bytes): the Skills
/// Store's key as a build ships it (#3692). Nothing else on a line, no
/// blank line, and at least one key; a signature's fingerprint picks the
/// line.
pub fn read_public_keys(text: &str) -> Result<Vec<[u8; 32]>, Refused> {
    let keys = text
        .lines()
        .map(|line| match line.len() {
            44 => read_public_key(line),
            _ => Err(refused("a line is not one key in base64")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() || keys.len() > MOST_PUBLIC_KEYS {
        return Err(refused(format!(
            "{} keys, and it holds 1 to {MOST_PUBLIC_KEYS}, one a line",
            keys.len()
        )));
    }
    Ok(keys)
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
    seal_unchecked(manifest, key)
}

/// [`seal`] without the check: what a publisher with the key but not this
/// program could sign, for tests of what a lamp refuses.
pub(crate) fn seal_unchecked(manifest: &[u8], key: &Ed25519KeyPair) -> Result<Vec<u8>, Refused> {
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

/// The manifest's bytes alone, once their signature with `key` is checked:
/// what `lamp-update-apply` reads, whatever updater the build needs.
pub fn signed(envelope: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, Refused> {
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
    Ok(manifest)
}

/// [`open_as`], with a build for a newer updater told apart: a round may
/// still take the newer `lamp-update` it carries, and only that (#1680).
pub fn open_or_too_old(envelope: &[u8], key: &[u8; 32], updater: u64) -> Result<Opened, Refused> {
    let manifest = signed(envelope, key)?;
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
        if unit.lamp == PRODUCT_LAMP && manifest.min_updater.unwrap_or(1) < PRODUCT_UNITS_UPDATER {
            return Err(refused(format!(
                "a build with product units needs min_updater {PRODUCT_UNITS_UPDATER} or more"
            )));
        }
        // A template only in its manifest form: never with its `@`.
        let companion = unit.lamp == PRODUCT_LAMP
            && !unit.unit.contains('@')
            && crate::companion::kind(&unit.on_lamp()).is_some();
        if !is_token(&unit.lamp) || !(is_unit_name(&unit.unit) || companion) {
            return Err(refused(format!(
                "{:?} of {:?} is not a unit of a lamp",
                unit.unit, unit.lamp
            )));
        }
        if companion && manifest.min_updater.unwrap_or(1) < COMPANIONS_UPDATER {
            return Err(refused(format!(
                "a build with a part's templates, sockets or timers needs min_updater {COMPANIONS_UPDATER} or more"
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
    check_models(manifest)?;
    if !manifest.settings.is_empty() && manifest.min_updater.unwrap_or(1) < SETTINGS_UPDATER {
        return Err(refused(format!(
            "a build with settings needs min_updater {SETTINGS_UPDATER} or more"
        )));
    }
    check_settings(&manifest.settings)?;
    if let Some(plan) = &manifest.reset_plan {
        if manifest.min_updater.unwrap_or(1) < RESET_PLAN_UPDATER {
            return Err(refused(format!(
                "a build with the reset plan needs min_updater {RESET_PLAN_UPDATER} or more"
            )));
        }
        check_reset_plan(plan)?;
    }
    if !manifest.image.is_empty() && manifest.min_updater.unwrap_or(1) < IMAGE_UPDATER {
        return Err(refused(format!(
            "a build with the image's files needs min_updater {IMAGE_UPDATER} or more"
        )));
    }
    if let Some(file) = manifest.image.iter().find(|file| {
        manifest
            .settings
            .iter()
            .any(|setting| setting.path == file.path)
    }) {
        return Err(refused(format!(
            "{} is both a setting and the image's",
            file.path
        )));
    }
    check_image(&manifest.image)
}

fn check_models(manifest: &Manifest) -> Result<(), Refused> {
    if manifest.models.is_empty() {
        return Ok(());
    }
    if manifest.models.len() > MOST_MODELS {
        return Err(refused(format!("more than {MOST_MODELS} models")));
    }
    if manifest.min_updater.unwrap_or(1) < MODELS_UPDATER {
        return Err(refused(format!(
            "a build with models needs min_updater {MODELS_UPDATER} or more"
        )));
    }
    let mut paths = std::collections::BTreeSet::new();
    for model in &manifest.models {
        if !is_model_path(&model.path) {
            return Err(refused(format!(
                "{:?} is not a model's path under {MODELS_ROOT}",
                model.path
            )));
        }
        if !paths.insert(model.path.as_str()) {
            return Err(refused(format!("{} is listed twice", model.path)));
        }
        if !is_file_url(&model.url) {
            return Err(refused(format!(
                "{}'s URL is not an https:// or file:/// URL",
                model.path
            )));
        }
        check_hash_and_size(&model.path, &model.sha256, model.bytes, MOST_FILE_BYTES)?;
        if let Some(archive) = &model.archive {
            check_hash_and_size(&model.url, &archive.sha256, archive.bytes, MOST_FILE_BYTES)?;
            if archive.member.is_empty()
                || archive.member.starts_with(['/', '-'])
                || archive.member.split('/').any(|part| part == "..")
            {
                return Err(refused(format!(
                    "{}'s archive member is not a plain path",
                    model.path
                )));
            }
        }
    }
    Ok(())
}

/// A model's path: absolute, under [`MODELS_ROOT`], neither the store nor
/// in it, nor in a build's folder (a part with a build's 64-hex name) nor a
/// `current` link itself; plain parts, a file's name at the end.
pub fn is_model_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(MODELS_ROOT) else {
        return false;
    };
    path.len() < 512
        && !path.starts_with(STORE)
        && path != STORE.trim_end_matches('/')
        && !rest.is_empty()
        && !path.ends_with('/')
        && rest.rsplit('/').next() != Some(crate::unit::CURRENT)
        && rest.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with('.')
                && !(part.len() == 64 && is_lower_hex(part))
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

fn is_file_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("file:///"));
    rest.is_some_and(|rest| {
        !url.ends_with('/')
            && !rest.contains("..")
            && !rest.contains(['?', '#', '\\', ' ', '@'])
            && rest.len() < 1024
    })
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

    /// The reviewer on #1945, 21:43: the image's files' one guard on what
    /// may be written as root, and its other refusals.
    #[test]
    fn an_image_file_outside_the_list_listed_twice_or_not_its_sha_is_refused() {
        let good = ImageFile {
            variant: "standard".into(),
            path: "/etc/asound.conf".into(),
            text: "asound".into(),
            sha256: sha256_hex(b"asound"),
            shipped: Vec::new(),
        };
        assert_eq!(check_image(std::slice::from_ref(&good)), Ok(()));
        let refused = |image: &[ImageFile]| check_image(image).unwrap_err().0;
        for path in [
            "/etc/sudoers.d/lamp",
            "/etc/asound.conf.d/x",
            "/etc/../etc/asound.conf",
        ] {
            let outside = ImageFile {
                path: path.into(),
                ..good.clone()
            };
            assert!(
                refused(&[outside]).contains("is not one of the image's files"),
                "{path}"
            );
        }
        assert!(refused(&[good.clone(), good.clone()]).contains("listed twice"));
        let wrong = ImageFile {
            sha256: sha256_hex(b"other"),
            ..good.clone()
        };
        assert!(refused(&[wrong]).contains("its text is not its SHA-256"));
    }

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
            models: Vec::new(),
            settings: Vec::new(),
            image: Vec::new(),
            reset_plan: None,
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
    fn a_parts_templates_sockets_and_timers_are_only_the_products_and_need_level_seven() {
        let key = throwaway();
        let mut product = manifest();
        product.units[0].lamp = PRODUCT_LAMP.into();
        product.units[0].unit = "lamp-setup-wifi.template.service".into();
        assert_eq!(product.units[0].on_lamp(), "lamp-setup-wifi@.service");
        assert_eq!(
            manifest_name("lamp-setup-wifi@.service"),
            "lamp-setup-wifi.template.service"
        );
        assert_eq!(lamp_name("lamp-voice.service"), "lamp-voice.service");
        for level in [Some(PRODUCT_UNITS_UPDATER), Some(COMPANIONS_UPDATER - 1)] {
            product.min_updater = level;
            let why = parse(&serde_json::to_vec(&product).unwrap()).unwrap_err().0;
            assert!(why.contains("timers needs min_updater 7"), "{why}");
        }
        product.min_updater = Some(COMPANIONS_UPDATER);
        for name in [
            "lamp-setup-wifi.template.service",
            "lamp-setup-wifi.socket",
            "lamp-records.timer",
        ] {
            product.units[0].unit = name.into();
            parse(&serde_json::to_vec(&product).unwrap()).unwrap();
            // A lamp's own folder carries services only.
            let mut own = product.clone();
            own.units[0].lamp = "lamp-4ace".into();
            let why = parse(&serde_json::to_vec(&own).unwrap()).unwrap_err().0;
            assert!(why.contains("is not a unit of a lamp"), "{why}");
        }
        // A release asset's name can't hold `@`: a template is only ever
        // named in its manifest form.
        for name in [
            "lampos.target",
            "lamp-x@1.service",
            "lampos-parts.slice",
            "lamp-setup-wifi@.service",
        ] {
            product.units[0].unit = name.into();
            let why = parse(&serde_json::to_vec(&product).unwrap()).unwrap_err().0;
            assert!(why.contains("is not a unit of a lamp"), "{name}: {why}");
        }
        // An updater of level 6 climbs first.
        product.units[0].unit = "lamp-records.timer".into();
        let envelope = seal(&serde_json::to_vec(&product).unwrap(), &key).unwrap();
        let why = open_as(&envelope, &public(&key), COMPANIONS_UPDATER - 1)
            .unwrap_err()
            .0;
        assert!(why.contains("needs updater 7"), "{why}");
    }

    #[test]
    fn product_units_require_the_signed_updater_level_before_older_readers_select_them() {
        let key = throwaway();
        let original = manifest();
        let original_bytes = serde_json::to_vec(&original).unwrap();
        assert_eq!(parse(&original_bytes).unwrap(), original);
        assert!(
            !String::from_utf8(original_bytes)
                .unwrap()
                .contains("min_updater")
        );

        let mut product = original;
        product.units[0].lamp = PRODUCT_LAMP.into();
        for level in [None, Some(PRODUCT_UNITS_UPDATER - 1)] {
            product.min_updater = level;
            let why = parse(&serde_json::to_vec(&product).unwrap()).unwrap_err().0;
            assert!(why.contains("product units needs min_updater 6"), "{why}");
        }
        product.min_updater = Some(PRODUCT_UNITS_UPDATER);
        let bytes = serde_json::to_vec(&product).unwrap();
        let envelope = seal(&bytes, &key).unwrap();
        let why = open_as(&envelope, &public(&key), PRODUCT_UNITS_UPDATER - 1)
            .unwrap_err()
            .0;
        assert!(why.contains("needs updater 6"), "{why}");
        assert_eq!(open(&envelope, &public(&key)).unwrap(), (product, bytes));

        let mut changed: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        let mut payload: serde_json::Value = serde_json::from_slice(
            &STANDARD
                .decode(changed["manifest"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        payload["min_updater"] = 5.into();
        changed["manifest"] = STANDARD
            .encode(serde_json::to_vec(&payload).unwrap())
            .into();
        assert_eq!(
            open_as(&serde_json::to_vec(&changed).unwrap(), &public(&key), 5)
                .unwrap_err()
                .0,
            "the signature does not match the manifest"
        );
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
    fn models_are_read_only_with_the_updater_they_need_and_plain_paths() {
        // None: not written, the manifest as before the field.
        let bytes = serde_json::to_vec(&manifest()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("models"));
        let model = Model {
            path: "/opt/lampOS/eyes/models/face.onnx".into(),
            url: "https://github.com/autonomous-ai/lampOS/releases/download/models-2026-10-16/face.onnx"
                .into(),
            bytes: 4,
            sha256: sha256_hex(b"face"),
            archive: None,
        };
        let mut with = manifest();
        with.models = vec![model.clone()];
        let why = parse(&serde_json::to_vec(&with).unwrap()).unwrap_err().0;
        assert!(why.contains("min_updater 2"), "{why}");
        with.min_updater = Some(MODELS_UPDATER);
        let bytes = serde_json::to_vec(&with).unwrap();
        assert_eq!(parse(&bytes).unwrap(), with);
        // An updater before the field climbs instead of reading it.
        assert!(too_old(&bytes, 1).is_some());
        let in_a_build = format!("/opt/lampOS/eyes/{}/face.onnx", "a".repeat(64));
        for path in [
            "/etc/passwd",
            "/opt/lampOS/models/abc",
            "/opt/lampOS/models",
            in_a_build.as_str(),
            "/opt/lampOS/eyes/current",
            "/opt/lampOS/eyes/../../etc/passwd",
            "/opt/lampOS/eyes/models/",
            "opt/lampOS/eyes/face.onnx",
        ] {
            let mut bad = with.clone();
            bad.models[0].path = path.into();
            assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err(), "{path}");
        }
        let mut twice = with.clone();
        twice.models.push(model);
        assert!(parse(&serde_json::to_vec(&twice).unwrap()).is_err());
    }

    #[test]
    fn settings_are_read_only_with_the_updater_they_need_and_their_own_paths() {
        let plain = serde_json::to_vec(&manifest()).unwrap();
        assert!(!String::from_utf8_lossy(&plain).contains("settings"));
        let setting = Setting::new("/etc/lampOS/expression/expression.json", "{}\n");
        let mut with = manifest();
        with.settings = vec![setting.clone()];
        let why = parse(&serde_json::to_vec(&with).unwrap()).unwrap_err().0;
        assert!(why.contains("min_updater 3"), "{why}");
        with.min_updater = Some(SETTINGS_UPDATER);
        assert_eq!(parse(&serde_json::to_vec(&with).unwrap()).unwrap(), with);
        for path in [
            "/etc/lampOS/records.json",
            "/etc/tmpfiles.d/lampos.conf",
            "/etc/lampOS/controls/controls-pins.json",
            "/var/lib/lampOS/skills/welcome-back/lights/warm_glow.toml",
        ] {
            assert!(is_setting_path(path), "{path}");
        }
        // Never a key, setup's answers, the lamp's own poses, the update key,
        // nor a path out of its folder.
        for path in [
            "/etc/lampOS/voice/credential",
            "/etc/lampOS/voice/provider.json",
            "/etc/lampOS/policy/jev.key",
            "/etc/lampOS/update/key.pub",
            "/etc/lampOS/zone.env",
            "/etc/lampOS/setup/serve.json",
            "/etc/lampOS/body/lamp.env",
            "/etc/lampOS/person/name",
            "/etc/lampOS/controls/../update/key.pub",
            "/etc/lampOS/controls/.hidden",
            "/etc/lampOS/controls/",
            "/etc/tmpfiles.d/other.conf",
            "/etc/passwd",
        ] {
            assert!(!is_setting_path(path), "{path}");
            let mut bad = with.clone();
            bad.settings[0].path = path.into();
            assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err(), "{path}");
        }
        let mut wrong = with.clone();
        wrong.settings[0].text = Some("{\"other\":1}".into());
        assert!(parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
        let mut open = with.clone();
        open.settings[0].mode = Some("0666".into());
        assert!(parse(&serde_json::to_vec(&open).unwrap()).is_err());
        let mut gone = with.clone();
        gone.settings[0] = Setting {
            path: setting.path.clone(),
            text: None,
            sha256: None,
            mode: None,
            removed: true,
        };
        assert!(parse(&serde_json::to_vec(&gone).unwrap()).is_ok());
        let mut twice = with.clone();
        twice.settings.push(setting);
        assert!(parse(&serde_json::to_vec(&twice).unwrap()).is_err());
    }

    /// `lamp-update-apply` is as old as the lamp's install: it reads a
    /// setting with a field it does not know, and the signed bytes of a
    /// build for any updater.
    #[test]
    fn a_setting_with_a_field_it_does_not_know_is_still_read() {
        let text = r#"{"path":"/etc/lampOS/records.json","text":"x","sha256":"2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881","mode":"0644","owner":"lampos"}"#;
        let setting: Setting = serde_json::from_str(text).unwrap();
        assert_eq!(setting.text.as_deref(), Some("x"));
        assert!(check_settings(&[setting]).is_ok());
        let key = throwaway();
        let mut newer = manifest();
        newer.min_updater = Some(UPDATER + 5);
        newer.files.push(File {
            name: "lamp-update".into(),
            sha256: sha256_hex(b"u"),
            size: 1,
        });
        let bytes = serde_json::to_vec(&newer).unwrap();
        let envelope = seal(&bytes, &key).unwrap();
        assert_eq!(signed(&envelope, &public(&key)).unwrap(), bytes);
        assert!(signed(&envelope, &public(&throwaway())).is_err());
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

    /// #806: the reset plan is a field of its own, not a setting, so a
    /// lamp-update-back from before it, which refuses a setting path it
    /// does not know, passes it over. It needs updater 5, its own path,
    /// its text with its SHA-256, and 0644.
    #[test]
    fn the_reset_plan_is_read_only_with_its_updater_and_its_own_path() {
        let plain = serde_json::to_vec(&manifest()).unwrap();
        assert!(!String::from_utf8_lossy(&plain).contains("reset_plan"));
        let mut with = manifest();
        with.reset_plan = Some(Box::new(Setting::new(RESET_PLAN, "{}\n")));
        with.min_updater = Some(IMAGE_UPDATER);
        let why = parse(&serde_json::to_vec(&with).unwrap()).unwrap_err().0;
        assert!(why.contains("min_updater 5"), "{why}");
        with.min_updater = Some(RESET_PLAN_UPDATER);
        assert_eq!(parse(&serde_json::to_vec(&with).unwrap()).unwrap(), with);
        assert!(!is_setting_path(RESET_PLAN));
        let mut elsewhere = with.clone();
        elsewhere.reset_plan = Some(Box::new(Setting::new(
            "/etc/lampOS/setup/serve.json",
            "{}\n",
        )));
        assert!(parse(&serde_json::to_vec(&elsewhere).unwrap()).is_err());
        let mut wrong = with.clone();
        wrong.reset_plan.as_mut().unwrap().sha256 = Some("0".repeat(64));
        assert!(parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
        let mut closed = with.clone();
        closed.reset_plan.as_mut().unwrap().mode = Some("0600".into());
        assert!(parse(&serde_json::to_vec(&closed).unwrap()).is_err());
        // An updater one level behind refuses it for its updater, so it
        // climbs, never for a field it does not know.
        let key = throwaway();
        let envelope = seal(&serde_json::to_vec(&with).unwrap(), &key).unwrap();
        let why = open_as(&envelope, &public(&key), RESET_PLAN_UPDATER - 1)
            .unwrap_err()
            .0;
        assert!(
            why.contains(&format!("needs updater {RESET_PLAN_UPDATER}")),
            "{why}"
        );
    }
}
