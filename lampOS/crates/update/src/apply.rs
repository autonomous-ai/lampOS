//! The product's settings and tmpfiles a build carries, written at the
//! switch by `lamp-update-apply` (#1680, docs/updates.md). The round enables
//! them with `LAMP_UPDATE_APPLY=on`, or the authenticated managed-product
//! default. An explicit off still prevents the round requesting any write.
//!
//! A round runs in a sandbox that writes only `/opt/lampOS`,
//! `/etc/systemd/system` and its records. So at the switch it stages a
//! request and the build's signed envelope in its records, and starts
//! `lamp-update-apply.service`: a one-shot that runs `lamp-update-back
//! apply`, the copy only `install` writes, never one from `update/current`,
//! which a round can write. It checks the envelope's signature again with
//! the lamp's own key, and writes nothing a build's settings may not name
//! (`manifest::is_setting_path`).
//!
//! Each setting is one of three: the build's before (its SHA-256 in the
//! state's `settings`, as install or the last apply wrote it), the build's
//! now (the manifest), and the lamp's now (the file). Only one still the
//! build's before, or one never there and never written, is written; one
//! the lamp changed or removed is kept, and said. Before the first write it
//! writes `apply/back.json`, each file's bytes and mode as they were, and
//! putting the build back restores each one byte for byte, unless the lamp
//! changed it again since.
//!
//! What it writes for the round is `apply/done.json`: each record the state
//! takes. It never writes `state.json` itself, which may be of a newer
//! updater than it.
//!
//! The build's reset plan (`reset_plan`, #806) is the product's and never
//! the person's: wherever the lamp has a plan, the build's is written over
//! it, the old one kept first in the records as
//! `reset-plan.json.before-<build>` unless it is the plan recorded. A lamp
//! with no plan gets none: one set up by hand would have its 5 s reset armed.
//! A copy older than the field ignores it, and the round says so.
//!
//! With the request's `image`, the image's own files for the lamp's
//! variant are written the same way (crate::image, #1369), where the lamp's
//! is still one the image shipped; then systemd and udev are reloaded, and
//! journald never is. A copy older than the field ignores it.
//!
//! The request, `back.json` and `done.json` are a contract for good, as
//! `self.json` is: their fields keep their meaning, and a field one side
//! does not know is ignored.
use crate::{
    image,
    manifest::{self, ImageFile, Setting},
    records::{State, sync_folder, unix_ms, write_whole},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// The switch, in lamp-update.service's environment: `on`, or off.
pub const SWITCH: &str = "LAMP_UPDATE_APPLY";
/// The one-shot a round starts.
pub const UNIT: &str = "lamp-update-apply.service";
/// The version of the files this program writes.
pub const VERSION: u32 = 1;
/// Its folder in the records.
pub const FOLDER: &str = "apply";
const REQUEST: &str = "request.json";
const ENVELOPE: &str = "envelope.json";
const BACK: &str = "back.json";
const DONE: &str = "done.json";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What a round asks of `lamp-update-apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// The staged build's settings written.
    Apply,
    /// What the apply of that build wrote put back.
    Back,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub op: Op,
    /// The build: the envelope's, and `back.json`'s.
    pub sequence: u64,
    /// The image's files are written too (`LAMP_UPDATE_IMAGE=on`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub image: bool,
}

/// One file as it was before an apply wrote it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Before {
    pub path: String,
    /// Its bytes in base64, or none: it was not there.
    #[serde(default)]
    pub bytes: Option<String>,
    #[serde(default)]
    pub mode: Option<u32>,
    /// Its record in the state before, if any.
    #[serde(default)]
    pub record: Option<String>,
    /// The SHA-256 the apply wrote, or none: it removed the file.
    #[serde(default)]
    pub wrote: Option<String>,
    /// The folders the apply made for it, the deepest first: removed again
    /// when it is put back as never there, if still empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub made: Vec<String>,
}

/// `back.json`: what an apply changed, as it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Back {
    pub version: u32,
    pub sequence: u64,
    pub files: Vec<Before>,
}

/// `done.json`: each record the state takes, by path; none removes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    pub version: u32,
    pub op: Op,
    pub sequence: u64,
    pub records: BTreeMap<String, Option<String>>,
}

/// What a setting needs on this lamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Written: the lamp has the build's before, or never had it.
    Write,
    /// Removed: the build no longer has it, and the lamp's is the build's.
    Remove,
    /// Already the build's now: only recorded.
    Record,
    /// The lamp's own: kept, with why.
    Keep(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub setting: Setting,
    pub act: Act,
}

fn on_root(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

fn now_sha(root: &Path, path: &str) -> io::Result<Option<String>> {
    match fs::read(on_root(root, path)) {
        Ok(bytes) => Ok(Some(manifest::sha256_hex(&bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Every SHA-256 install has written at `/etc/tmpfiles.d/lampos.conf`
/// (deploy/product/lampos.conf in git). An install before #1680 wrote it
/// without a record in the state's `settings`: on such a lamp one of these
/// is still the build's before, not the lamp's own. Never remove one.
pub const TMPFILES_SHIPPED: &[&str] = &[
    "e2ec764a900d6e7f3f338b2e9084b0f49f6fd3600dbfea91e0385426c01670c8",
    "00b4b031eaa3f1dd284388e780d0b78256c15c8e6cec4e1a6f3fec101f455860",
    "ec07938364e3354c7288b42c1bf779eee84b17795426c891902bed431cfd5ca3",
    "1ffdc449d1c97c310ab18625eed43d4968d2fdaf67faf82add29aed62e6740a0",
    "3f50bc935ba8e21498e8c47d44ae52b2475bab7d429bb17738f9bde07bda0838",
    "7c5479421960ae2c349b9b0a8e4d14bd711377100336979acab639fb9d6b138c",
    "3d5ae5d6c2351dbebc0f5e142441f1637f405c8258b3a6beb30f6f848571bf24",
];

/// Whether `sha` at `path` is the build's before: its record, the one
/// install was about to write, or, for the tmpfiles with no record, one an
/// install shipped.
fn builds_before(
    path: &str,
    sha: &str,
    recorded: &BTreeMap<String, String>,
    writing: &BTreeMap<String, String>,
) -> bool {
    recorded.get(path).map(String::as_str) == Some(sha)
        || writing.get(path).map(String::as_str) == Some(sha)
        || (path == crate::install::TMPFILES_PATH
            && !recorded.contains_key(path)
            && TMPFILES_SHIPPED.contains(&sha))
}

/// What each setting needs, from the three: the build's before
/// (`recorded`, or `writing` for one install was about to write, or for
/// an unrecorded tmpfiles [`TMPFILES_SHIPPED`], or for the image's files
/// any SHA-256 in `shipped`), the build's now, and the
/// lamp's now. Nothing to do is left out.
pub fn plan(
    root: &Path,
    settings: &[Setting],
    recorded: &BTreeMap<String, String>,
    writing: &BTreeMap<String, String>,
    shipped: &BTreeMap<String, Vec<String>>,
) -> io::Result<Vec<Step>> {
    let mut steps = Vec::new();
    for setting in settings {
        let now = now_sha(root, &setting.path)
            .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", setting.path)))?;
        let builds = |sha: &String| {
            builds_before(&setting.path, sha, recorded, writing)
                || shipped
                    .get(&setting.path)
                    .is_some_and(|shipped| shipped.contains(sha))
        };
        let act = match (&now, setting.removed, &setting.sha256) {
            (None, true, _) => None,
            (Some(now), true, _) if builds(now) => Some(Act::Remove),
            (Some(_), true, _) => Some(Act::Keep("this lamp's own".into())),
            (Some(now), false, Some(new)) if now == new => {
                (recorded.get(&setting.path) != Some(new)).then_some(Act::Record)
            }
            (None, false, _) if recorded.contains_key(&setting.path) => {
                Some(Act::Keep("removed on this lamp".into()))
            }
            (None, false, _) => Some(Act::Write),
            (Some(now), false, _) if builds(now) => Some(Act::Write),
            (Some(_), false, _) => Some(Act::Keep("this lamp's own".into())),
        };
        if let Some(act) = act {
            steps.push(Step {
                setting: setting.clone(),
                act,
            });
        }
    }
    Ok(steps)
}

/// What the build's reset plan needs on this lamp (#806): none where the
/// lamp has no plan; recorded where it is the build's already; written over
/// any other, the lamp's own or a past build's alike, never kept.
pub fn reset_plan_step(
    root: &Path,
    plan: &Setting,
    recorded: &BTreeMap<String, String>,
) -> io::Result<Option<Step>> {
    let now = now_sha(root, &plan.path)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", plan.path)))?;
    let act = match now {
        None => None,
        Some(now) if Some(&now) == plan.sha256.as_ref() => {
            (recorded.get(&plan.path) != Some(&now)).then_some(Act::Record)
        }
        Some(_) => Some(Act::Write),
    };
    Ok(act.map(|act| Step {
        setting: plan.clone(),
        act,
    }))
}

/// The lamp's reset plan, kept in `records` before the build's is written
/// over it, as `reset-plan.json.before-<sequence>` (with the time after it
/// if one of another plan is there), and said: whether it is a plan install
/// once shipped ([`crate::install::RESET_PLAN_SHIPPED`]) or not. None when
/// it is the plan recorded, which a build wrote, or it is not there.
pub fn keep_reset_plan(
    root: &Path,
    records: &Path,
    sequence: u64,
    recorded: Option<&String>,
    say: &mut dyn FnMut(&str),
) -> io::Result<Option<PathBuf>> {
    let at = on_root(root, manifest::RESET_PLAN);
    let bytes = match fs::read(&at) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let sha = manifest::sha256_hex(&bytes);
    if recorded == Some(&sha) {
        return Ok(None);
    }
    let mut kept = records.join(format!("reset-plan.json.before-{sequence}"));
    if fs::read(&kept).is_ok_and(|there| there != bytes) {
        kept = records.join(format!("reset-plan.json.before-{sequence}-{}", unix_ms()));
    }
    write_whole(&kept, &bytes, 0o600)?;
    let whose = if crate::install::RESET_PLAN_SHIPPED.contains(&sha.as_str()) {
        "a plan an earlier build shipped"
    } else {
        "a plan no build shipped"
    };
    say(&format!(
        "{} was {whose} ({}…): kept in {} before the build's is written",
        manifest::RESET_PLAN,
        &sha[..12],
        kept.display()
    ));
    Ok(Some(kept))
}

/// The paths a plan writes or removes.
pub fn writes(steps: &[Step]) -> Vec<String> {
    steps
        .iter()
        .filter(|step| matches!(step.act, Act::Write | Act::Remove))
        .map(|step| step.setting.path.clone())
        .collect()
}

fn folder(records: &Path) -> PathBuf {
    records.join(FOLDER)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_whole(path, &bytes, 0o600)
}

/// Written by the round: what it asks, with the build's envelope for an
/// apply. A `done.json` left before is gone first, so only this request's
/// is ever read.
pub fn stage(
    records: &Path,
    op: Op,
    sequence: u64,
    envelope: Option<&[u8]>,
    image: bool,
) -> io::Result<()> {
    let folder = folder(records);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)?;
    remove(&folder.join(DONE))?;
    if let Some(envelope) = envelope {
        write_whole(&folder.join(ENVELOPE), envelope, 0o600)?;
    }
    write_json(
        &folder.join(REQUEST),
        &Request {
            version: VERSION,
            op,
            sequence,
            image,
        },
    )
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// What the staged request's matching `done.json` records, taken into `state`.
/// Whether there was a receipt. The caller saves the state, then [`finish`]es.
/// A waiting parent that already knows what it asked uses [`merge_for`], so
/// a rewritten staged request cannot change the identity it expects.
pub fn merge(records: &Path, state: &mut State) -> io::Result<bool> {
    let Some(done) = read_json::<Done>(&folder(records).join(DONE))? else {
        return Ok(false);
    };
    let request = read_json::<Request>(&folder(records).join(REQUEST))?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "done.json has no staged settings request",
        )
    })?;
    merge_done(state, done, request.op, request.sequence)
}

/// Merge only a receipt for the operation and build the waiting caller asked.
/// Neither `state` nor the staged files change on a mismatch. This binds the
/// identity; it does not authenticate the helper or verify its record values.
pub fn merge_for(records: &Path, state: &mut State, op: Op, sequence: u64) -> io::Result<bool> {
    let Some(done) = read_json::<Done>(&folder(records).join(DONE))? else {
        return Ok(false);
    };
    merge_done(state, done, op, sequence)
}

fn merge_done(state: &mut State, done: Done, op: Op, sequence: u64) -> io::Result<bool> {
    // These fields keep their meaning across versions of the permanent
    // apply contract. A newer version or extra field is still read past.
    if done.op != op || done.sequence != sequence {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "done.json is not for the requested settings operation and build",
        ));
    }
    for (path, record) in done.records {
        match record {
            Some(sha) => state.settings.insert(path, sha),
            None => state.settings.remove(&path),
        };
    }
    Ok(true)
}

/// Whether an apply of build `sequence` may have written anything: its
/// `back.json` is there, or cannot be read. Nothing is written before it.
pub fn wrote(records: &Path, sequence: u64) -> bool {
    match read_json::<Back>(&folder(records).join(BACK)) {
        Ok(Some(back)) => back.sequence == sequence && !back.files.is_empty(),
        Ok(None) => false,
        Err(_) => true,
    }
}

/// The request, its envelope and its `done.json` gone. `back.json` stays:
/// the way back by hand.
pub fn finish(records: &Path) -> io::Result<()> {
    let folder = folder(records);
    for name in [DONE, REQUEST, ENVELOPE] {
        remove(&folder.join(name))?;
    }
    if folder.exists() {
        sync_folder(&folder)?;
    }
    Ok(())
}

/// The little of a manifest `lamp-update-apply` reads, leniently: any
/// newer manifest keeps these two readable.
#[derive(Deserialize)]
struct Signed {
    sequence: u64,
    #[serde(default)]
    settings: Vec<Setting>,
    #[serde(default)]
    image: Vec<ImageFile>,
    #[serde(default)]
    reset_plan: Option<Setting>,
}

/// The little of the state it reads: the records, never written by it.
#[derive(Default, Deserialize)]
struct Recorded {
    #[serde(default)]
    settings: BTreeMap<String, String>,
    #[serde(default)]
    settings_writing: BTreeMap<String, String>,
}

/// `lamp-update-back apply`: the request the round staged, done. No
/// request, nothing. `command` runs one program with its arguments:
/// `systemd-tmpfiles --create` when the tmpfiles were written or put back,
/// and what the image's files need after (crate::image::after). Every
/// command passes [`image::guard`] first: journald is never one.
pub fn run(
    root: &Path,
    records: &Path,
    key: &[u8; 32],
    command: &mut dyn FnMut(&[&str]) -> Result<(), String>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let mut guarded = |parts: &[&str]| {
        image::guard(parts)?;
        command(parts)
    };
    let folder = folder(records);
    let failed = |what: &Path, error: io::Error| format!("{}: {error}", what.display());
    let Some(request) = read_json::<Request>(&folder.join(REQUEST))
        .map_err(|e| failed(&folder.join(REQUEST), e))?
    else {
        say("nothing is asked: no request");
        return Ok(());
    };
    match request.op {
        Op::Apply => apply(root, &folder, records, key, &request, &mut guarded, say),
        Op::Back => back(root, &folder, request.sequence, &mut guarded, say),
    }
}

/// `systemd-tmpfiles --create` of the product's tmpfiles.
fn tmpfiles(command: &mut dyn FnMut(&[&str]) -> Result<(), String>) -> Result<(), String> {
    command(&[
        "systemd-tmpfiles",
        "--create",
        crate::install::TMPFILES_PATH,
    ])
}

/// What the image's files written or put back need: each command run, one
/// whose failure is only said, and what waits for the next boot said.
fn after_image(
    paths: &[String],
    command: &mut dyn FnMut(&[&str]) -> Result<(), String>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    for (parts, only_said) in image::after(paths) {
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        match command(&parts) {
            Ok(()) => {}
            Err(why) if only_said => say(&format!("{}: {why}", parts.join(" "))),
            Err(why) => return Err(why),
        }
    }
    for path in paths {
        if let Some(line) = image::later(path) {
            say(&line);
        }
    }
    Ok(())
}

/// Where an image unit is staged for `systemd-analyze verify`.
pub const VERIFYING: &str = "/run/lampos-update-apply/";

/// `systemd-analyze verify` of an image unit (not a drop-in) before it is
/// written: staged under [`VERIFYING`] by its own name, and removed after.
fn verify_unit(
    root: &Path,
    path: &str,
    text: &str,
    command: &mut dyn FnMut(&[&str]) -> Result<(), String>,
) -> Result<(), String> {
    let Some(name) = path
        .strip_prefix(crate::install::UNITS)
        .filter(|name| !name.contains('/'))
    else {
        return Ok(());
    };
    let staged = format!("{VERIFYING}{name}");
    put(root, &staged, text.as_bytes(), 0o644)?;
    let verified = command(&["systemd-analyze", "verify", &staged]);
    remove(&on_root(root, &staged)).map_err(|error| failed_at(&staged, error))?;
    verified.map_err(|why| format!("{path} is not written, systemd can't load it: {why}"))
}

fn is_image_path(path: &str) -> bool {
    manifest::IMAGE_FILES.contains(&path)
}

fn apply(
    root: &Path,
    folder: &Path,
    records: &Path,
    key: &[u8; 32],
    request: &Request,
    command: &mut dyn FnMut(&[&str]) -> Result<(), String>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let sequence = request.sequence;
    let envelope =
        fs::read(folder.join(ENVELOPE)).map_err(|error| format!("the envelope: {error}"))?;
    let bytes = manifest::signed(&envelope, key).map_err(|why| why.0)?;
    let signed: Signed = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the manifest cannot be read: {error}"))?;
    if signed.sequence != sequence {
        return Err(format!(
            "the envelope is of build {}, and build {sequence} was asked",
            signed.sequence
        ));
    }
    manifest::check_settings(&signed.settings).map_err(|why| why.0)?;
    manifest::check_image(&signed.image).map_err(|why| why.0)?;
    if let Some(plan) = &signed.reset_plan {
        manifest::check_reset_plan(plan).map_err(|why| why.0)?;
    }
    let recorded: Recorded = read_json(&records.join("state.json"))
        .map_err(|error| format!("the state: {error}"))?
        .unwrap_or_default();
    let mut steps = plan(
        root,
        &signed.settings,
        &recorded.settings,
        &recorded.settings_writing,
        &BTreeMap::new(),
    )
    .map_err(|error| error.to_string())?;
    crate::head_touch::project(
        root,
        &root.join("etc/systemd/system"),
        &BTreeMap::new(),
        &signed.settings,
        &mut steps,
    )?;
    crate::head_touch::verify(
        root,
        &root.join("etc/systemd/system"),
        &BTreeMap::new(),
        &steps,
    )?;
    if let Some(plan) = &signed.reset_plan {
        steps.extend(
            reset_plan_step(root, plan, &recorded.settings).map_err(|error| error.to_string())?,
        );
    }
    let of_settings = steps.len();
    if request.image && !signed.image.is_empty() {
        match image::variant(root).map_err(|error| format!("{}: {error}", image::PROFILE))? {
            Some(variant) => {
                let (files, shipped) = image::for_variant(&signed.image, &variant);
                if files
                    .iter()
                    .any(|file| signed.settings.iter().any(|s| s.path == file.path))
                {
                    return Err("a path is both a setting and the image's".into());
                }
                steps.extend(
                    plan(root, &files, &recorded.settings, &BTreeMap::new(), &shipped)
                        .map_err(|error| error.to_string())?,
                );
            }
            None => say(&format!(
                "the image's files are not written: {} names no variant",
                image::PROFILE
            )),
        }
    }
    // Every path checked again before anything is written, as `back`
    // does: a setting's a setting's path, an image file one of
    // `IMAGE_FILES`, whatever the checks above let through.
    for (at, step) in steps.iter().enumerate() {
        let path = &step.setting.path;
        let allowed = if at < of_settings {
            manifest::is_setting_path(path) || path == manifest::RESET_PLAN
        } else {
            is_image_path(path)
        };
        if !allowed {
            return Err(format!("{path} is not a path this build may write"));
        }
    }
    // Each of the image's units checked by systemd before anything is
    // written: a unit it can't load would be a broken unit at boot.
    for step in &steps {
        if step.act == Act::Write && is_image_path(&step.setting.path) {
            verify_unit(
                root,
                &step.setting.path,
                step.setting.text.as_deref().unwrap_or_default(),
                command,
            )?;
        }
    }
    // The lamp's reset plan kept whole in the records, beside back.json,
    // which the next build's apply writes over (#806).
    if steps
        .iter()
        .any(|step| step.act == Act::Write && step.setting.path == manifest::RESET_PLAN)
    {
        keep_reset_plan(
            root,
            records,
            sequence,
            recorded.settings.get(manifest::RESET_PLAN),
            say,
        )
        .map_err(|error| failed_at(manifest::RESET_PLAN, error))?;
    }
    // Every file as it was, kept before the first is changed.
    let mut files = Vec::new();
    for step in &steps {
        let wrote = match step.act {
            Act::Write => step.setting.sha256.clone(),
            Act::Remove => None,
            _ => continue,
        };
        let at = on_root(root, &step.setting.path);
        let (bytes, mode) = match fs::read(&at) {
            Ok(bytes) => {
                let mode = fs::metadata(&at)
                    .map_err(|error| failed_at(&step.setting.path, error))?
                    .permissions()
                    .mode()
                    & 0o7777;
                (Some(STANDARD.encode(bytes)), Some(mode))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (None, None),
            Err(error) => return Err(failed_at(&step.setting.path, error)),
        };
        let made = if bytes.is_none() {
            missing_folders(root, &step.setting.path)
        } else {
            Vec::new()
        };
        files.push(Before {
            path: step.setting.path.clone(),
            bytes,
            mode,
            record: recorded.settings.get(&step.setting.path).cloned(),
            wrote,
            made,
        });
    }
    if !files.is_empty() {
        write_json(
            &folder.join(BACK),
            &Back {
                version: VERSION,
                sequence,
                files,
            },
        )
        .map_err(|error| format!("{BACK}: {error}"))?;
    }
    let mut records_now = BTreeMap::new();
    let mut tmpfiles_written = false;
    let mut image_written = Vec::new();
    for step in &steps {
        let path = &step.setting.path;
        match &step.act {
            Act::Write => {
                let text = step.setting.text.as_deref().unwrap_or_default();
                put(root, path, text.as_bytes(), step.setting.mode_bits())?;
                say(&format!("wrote {path}"));
                records_now.insert(path.clone(), step.setting.sha256.clone());
            }
            Act::Remove => {
                remove(&on_root(root, path)).map_err(|error| failed_at(path, error))?;
                say(&format!("removed {path}"));
                records_now.insert(path.clone(), None);
            }
            Act::Record => {
                records_now.insert(path.clone(), step.setting.sha256.clone());
            }
            Act::Keep(why) => {
                say(&format!("kept {path}: {why}; the build's is not written"));
                continue;
            }
        }
        tmpfiles_written |= path == crate::install::TMPFILES_PATH && step.act != Act::Record;
        if is_image_path(path) && step.act != Act::Record {
            image_written.push(path.clone());
        }
    }
    if tmpfiles_written && tmpfiles_there(root) {
        tmpfiles(command)?;
    }
    after_image(&image_written, command, say)?;
    write_json(
        &folder.join(DONE),
        &Done {
            version: VERSION,
            op: Op::Apply,
            sequence,
            records: records_now,
        },
    )
    .map_err(|error| format!("{DONE}: {error}"))
}

fn back(
    root: &Path,
    folder: &Path,
    sequence: u64,
    command: &mut dyn FnMut(&[&str]) -> Result<(), String>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let back = read_json::<Back>(&folder.join(BACK)).map_err(|error| format!("{BACK}: {error}"))?;
    let mut records_now = BTreeMap::new();
    let mut tmpfiles_back = false;
    let mut image_back = Vec::new();
    match back {
        Some(back) if back.sequence == sequence => {
            let mut troubles = Vec::new();
            for before in &back.files {
                let path = &before.path;
                if !manifest::is_setting_path(path)
                    && !is_image_path(path)
                    && path != manifest::RESET_PLAN
                {
                    troubles.push(format!("{path} is not a setting's path"));
                    continue;
                }
                let was = match &before.bytes {
                    Some(bytes) => match STANDARD.decode(bytes) {
                        Ok(bytes) => Some(manifest::sha256_hex(&bytes)),
                        Err(_) => {
                            troubles.push(format!("{path}: its bytes before are not base64"));
                            continue;
                        }
                    },
                    None => None,
                };
                match now_sha(root, path) {
                    Ok(now) if now == before.wrote => {}
                    Ok(now) if now == was => {
                        // Already as it was: a put-back cut short after
                        // restoring it. Its record is still put back.
                        say(&format!("{path} is already as it was"));
                        records_now.insert(path.clone(), before.record.clone());
                        remove_made(root, path, &before.made);
                        tmpfiles_back |= path == crate::install::TMPFILES_PATH;
                        if is_image_path(path) {
                            image_back.push(path.clone());
                        }
                        continue;
                    }
                    Ok(_) => {
                        say(&format!(
                            "kept {path}: changed on this lamp since build {sequence} wrote it"
                        ));
                        continue;
                    }
                    Err(error) => {
                        troubles.push(failed_at(path, error));
                        continue;
                    }
                }
                let restored = match &before.bytes {
                    Some(bytes) => STANDARD
                        .decode(bytes)
                        .map_err(|_| format!("{path}: its bytes before are not base64"))
                        .and_then(|bytes| put(root, path, &bytes, before.mode.unwrap_or(0o644))),
                    None => remove(&on_root(root, path))
                        .map_err(|error| failed_at(path, error))
                        .map(|()| remove_made(root, path, &before.made)),
                };
                match restored {
                    Ok(()) => {
                        say(&format!("put {path} back as it was"));
                        records_now.insert(path.clone(), before.record.clone());
                        tmpfiles_back |= path == crate::install::TMPFILES_PATH;
                        if is_image_path(path) {
                            image_back.push(path.clone());
                        }
                    }
                    Err(why) => troubles.push(why),
                }
            }
            if tmpfiles_back
                && tmpfiles_there(root)
                && let Err(why) = tmpfiles(command)
            {
                troubles.push(why);
            }
            if let Err(why) = after_image(&image_back, command, say) {
                troubles.push(why);
            }
            if !troubles.is_empty() {
                // Nothing is recorded: the round tries again from the journal.
                return Err(troubles.join("; "));
            }
        }
        _ => say(&format!(
            "nothing to put back: build {sequence} wrote no setting"
        )),
    }
    write_json(
        &folder.join(DONE),
        &Done {
            version: VERSION,
            op: Op::Back,
            sequence,
            records: records_now,
        },
    )
    .map_err(|error| format!("{DONE}: {error}"))
}

/// Whether the tmpfiles are there: `systemd-tmpfiles --create` of a
/// missing file fails, and one removed has nothing to create.
fn tmpfiles_there(root: &Path) -> bool {
    on_root(root, crate::install::TMPFILES_PATH).exists()
}

/// The folders of `path` not there yet, the deepest first, as paths on
/// the lamp.
fn missing_folders(root: &Path, path: &str) -> Vec<String> {
    Path::new(path)
        .ancestors()
        .skip(1)
        .take_while(|folder| {
            *folder != Path::new("/") && !on_root(root, &folder.to_string_lossy()).exists()
        })
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect()
}

/// The folders an apply made for `path`, removed the deepest first while
/// empty; one with anything in it now is the lamp's, and stays. Only a
/// folder of `path` inside the settings' folders is ever removed.
fn remove_made(root: &Path, path: &str, made: &[String]) {
    for folder in made {
        let of_path = Path::new(path)
            .ancestors()
            .any(|above| above == Path::new(folder));
        if !of_path
            || !manifest::is_setting_path(&format!("{folder}/x"))
            || fs::remove_dir(on_root(root, folder)).is_err()
        {
            break;
        }
    }
}

fn failed_at(path: &str, error: io::Error) -> String {
    format!("{path}: {error}")
}

/// `bytes` at `path`, whole, with `mode`; its folders made 0755.
fn put(root: &Path, path: &str, bytes: &[u8], mode: u32) -> Result<(), String> {
    let at = on_root(root, path);
    if let Some(parent) = at.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(|error| failed_at(path, error))?;
    }
    write_whole(&at, bytes, mode).map_err(|error| failed_at(path, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{rand::SystemRandom, signature::Ed25519KeyPair, signature::KeyPair};

    const EXPRESSION: &str = "/etc/lampOS/expression/expression.json";
    const CONTROLS: &str = "/etc/lampOS/controls/controls.json";
    const TMPFILES: &str = "/etc/tmpfiles.d/lampos.conf";

    struct Lamp {
        _folder: tempfile::TempDir,
        root: PathBuf,
        records: PathBuf,
        key: Ed25519KeyPair,
        said: Vec<String>,
        tmpfiles: Vec<String>,
        /// Every command run, whole.
        commands: Vec<String>,
        /// The request asks for the image's files.
        image: bool,
        /// A command that fails, by its start.
        failing: Option<String>,
    }

    impl Lamp {
        fn new() -> Self {
            let folder = tempfile::tempdir().unwrap();
            let root = folder.path().to_owned();
            let records = root.join("var/lib/lampos-update");
            fs::create_dir_all(&records).unwrap();
            let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            Self {
                _folder: folder,
                root,
                records,
                key: Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap(),
                said: Vec::new(),
                tmpfiles: Vec::new(),
                commands: Vec::new(),
                image: false,
                failing: None,
            }
        }

        fn write(&self, path: &str, text: &str) {
            let at = on_root(&self.root, path);
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(at, text).unwrap();
        }

        fn read(&self, path: &str) -> Option<String> {
            fs::read_to_string(on_root(&self.root, path)).ok()
        }

        /// install wrote these, and recorded them.
        fn installed(&self, files: &[(&str, &str)]) {
            let mut state = State::default();
            for (path, text) in files {
                self.write(path, text);
                state
                    .settings
                    .insert((*path).into(), manifest::sha256_hex(text.as_bytes()));
            }
            fs::write(
                self.records.join("state.json"),
                serde_json::to_vec(&state).unwrap(),
            )
            .unwrap();
        }

        fn state(&self) -> State {
            serde_json::from_slice(&fs::read(self.records.join("state.json")).unwrap()).unwrap()
        }

        fn envelope(&self, sequence: u64, settings: Vec<Setting>) -> Vec<u8> {
            self.envelope_with(sequence, settings, Vec::new())
        }

        fn envelope_with(
            &self,
            sequence: u64,
            settings: Vec<Setting>,
            image: Vec<ImageFile>,
        ) -> Vec<u8> {
            let min_updater = if image.is_empty() {
                manifest::SETTINGS_UPDATER
            } else {
                manifest::IMAGE_UPDATER
            };
            let manifest = manifest::Manifest {
                schema: 1,
                channel: "lamp-voice".into(),
                sequence,
                commit: "a".repeat(40),
                files_url: "https://channel.test/".into(),
                files: vec![manifest::File {
                    name: "lamp-update".into(),
                    sha256: manifest::sha256_hex(b"u"),
                    size: 1,
                }],
                units: Vec::new(),
                min_updater: Some(min_updater),
                models: Vec::new(),
                settings,
                image,
                reset_plan: None,
            };
            // Signed unchecked: what a lamp refuses is a publisher's to sign.
            manifest::seal_unchecked(&serde_json::to_vec(&manifest).unwrap(), &self.key).unwrap()
        }

        fn run(&mut self, op: Op, sequence: u64, envelope: Option<&[u8]>) -> Result<(), String> {
            stage(&self.records, op, sequence, envelope, self.image).unwrap();
            let key: [u8; 32] = self.key.public_key().as_ref().try_into().unwrap();
            let said = &mut self.said;
            let tmp = &mut self.tmpfiles;
            let commands = &mut self.commands;
            let failing = self.failing.clone();
            let result = run(
                &self.root,
                &self.records,
                &key,
                &mut |parts| {
                    let whole = parts.join(" ");
                    commands.push(whole.clone());
                    if parts[0] == "systemd-tmpfiles" {
                        tmp.push(parts[2].to_owned());
                    }
                    match &failing {
                        Some(start) if whole.starts_with(start.as_str()) => {
                            Err(format!("{whole}: failed"))
                        }
                        _ => Ok(()),
                    }
                },
                &mut |line| said.push(line.to_owned()),
            );
            // The round's part: the records taken, then the request gone.
            let mut state = self.state();
            if merge(&self.records, &mut state).unwrap() {
                fs::write(
                    self.records.join("state.json"),
                    serde_json::to_vec(&state).unwrap(),
                )
                .unwrap();
            }
            finish(&self.records).unwrap();
            result
        }
    }

    fn assert_mismatched_receipt_is_not_merged(op: Op, sequence: u64) {
        let lamp = Lamp::new();
        stage(&lamp.records, Op::Apply, 41, None, false).unwrap();
        write_json(
            &folder(&lamp.records).join(DONE),
            &Done {
                version: VERSION,
                op,
                sequence,
                records: [
                    (EXPRESSION.to_owned(), Some("wrong new record".into())),
                    (CONTROLS.to_owned(), None),
                ]
                .into(),
            },
        )
        .unwrap();
        let mut state = State::default();
        state
            .settings
            .insert(EXPRESSION.into(), "old expression".into());
        state
            .settings
            .insert(CONTROLS.into(), "old controls".into());
        state
            .later
            .insert("future_state".into(), serde_json::json!({"kept": true}));
        let before = state.clone();
        let request = fs::read(folder(&lamp.records).join(REQUEST)).unwrap();
        let done = fs::read(folder(&lamp.records).join(DONE)).unwrap();

        let result = merge(&lamp.records, &mut state);

        assert_eq!(state, before, "a mismatched receipt changed the state");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            fs::read(folder(&lamp.records).join(REQUEST)).unwrap(),
            request
        );
        assert_eq!(fs::read(folder(&lamp.records).join(DONE)).unwrap(), done);
    }

    #[test]
    fn a_done_for_another_operation_does_not_change_any_setting_record() {
        assert_mismatched_receipt_is_not_merged(Op::Back, 41);
    }

    #[test]
    fn a_done_for_another_sequence_does_not_change_any_setting_record() {
        assert_mismatched_receipt_is_not_merged(Op::Apply, 40);
    }

    #[test]
    fn an_explicit_expected_identity_does_not_follow_a_rewritten_staged_request() {
        let lamp = Lamp::new();
        stage(&lamp.records, Op::Back, 99, None, false).unwrap();
        write_json(
            &folder(&lamp.records).join(DONE),
            &Done {
                version: VERSION,
                op: Op::Apply,
                sequence: 41,
                records: [(EXPRESSION.to_owned(), Some("correct record".into()))].into(),
            },
        )
        .unwrap();
        let mut state = State::default();
        let before = state.clone();
        assert!(merge(&lamp.records, &mut state).is_err());
        assert_eq!(state, before);
        assert!(merge_for(&lamp.records, &mut state, Op::Apply, 41).unwrap());
        assert_eq!(state.settings[EXPRESSION], "correct record");
    }

    #[test]
    fn a_matching_done_keeps_the_permanent_forward_read_contract() {
        let lamp = Lamp::new();
        stage(&lamp.records, Op::Back, 41, None, false).unwrap();
        let mut request: serde_json::Value =
            serde_json::from_slice(&fs::read(folder(&lamp.records).join(REQUEST)).unwrap())
                .unwrap();
        request["version"] = serde_json::json!(VERSION + 1);
        request["later"] = serde_json::json!({"retained_meaning": true});
        write_json(&folder(&lamp.records).join(REQUEST), &request).unwrap();
        write_json(
            &folder(&lamp.records).join(DONE),
            &serde_json::json!({
                "version": VERSION + 1,
                "op": "back",
                "sequence": 41,
                "records": {EXPRESSION: "restored record", CONTROLS: null},
                "later": [1, 2, 3],
            }),
        )
        .unwrap();
        let mut state = State::default();
        state
            .settings
            .insert(CONTROLS.into(), "record to remove".into());
        state
            .later
            .insert("future_state".into(), serde_json::json!({"kept": true}));
        let mut explicit = state.clone();
        assert!(merge(&lamp.records, &mut state).unwrap());
        assert!(merge_for(&lamp.records, &mut explicit, Op::Back, 41).unwrap());
        assert_eq!(explicit, state);
        assert_eq!(state.settings[EXPRESSION], "restored record");
        assert!(!state.settings.contains_key(CONTROLS));
        assert_eq!(
            state.later["future_state"],
            serde_json::json!({"kept": true})
        );

        // With no receipt, the legacy helper and the explicit parent both do
        // nothing, even if there is no longer a staged request.
        finish(&lamp.records).unwrap();
        let before = state.clone();
        assert!(!merge(&lamp.records, &mut state).unwrap());
        assert!(!merge_for(&lamp.records, &mut state, Op::Back, 41).unwrap());
        assert_eq!(state, before);
    }

    #[test]
    fn an_unknown_done_operation_is_refused_without_changing_any_setting_record() {
        let lamp = Lamp::new();
        stage(&lamp.records, Op::Apply, 41, None, false).unwrap();
        write_json(
            &folder(&lamp.records).join(DONE),
            &serde_json::json!({
                "version": VERSION + 1,
                "op": "future_operation",
                "sequence": 41,
                "records": {EXPRESSION: "must not be merged"},
                "later": true,
            }),
        )
        .unwrap();
        let mut state = State::default();
        state
            .settings
            .insert(EXPRESSION.into(), "old record".into());
        let before = state.clone();
        assert!(merge(&lamp.records, &mut state).is_err());
        assert_eq!(state, before);
        assert!(merge_for(&lamp.records, &mut state, Op::Apply, 41).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn a_setting_still_the_builds_is_written_and_one_the_lamp_changed_is_kept() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1"), (CONTROLS, "controls 1")]);
        // The person's own pads: changed on the lamp since install wrote it.
        lamp.write(CONTROLS, "controls of this lamp");
        let envelope = lamp.envelope(
            2,
            vec![
                Setting::new(EXPRESSION, "expression 2"),
                Setting::new(CONTROLS, "controls 2"),
            ],
        );
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 2"));
        assert_eq!(
            lamp.read(CONTROLS).as_deref(),
            Some("controls of this lamp")
        );
        assert!(
            lamp.said
                .iter()
                .any(|line| line.starts_with(&format!("kept {CONTROLS}"))),
            "{:?}",
            lamp.said
        );
        let state = lamp.state();
        assert_eq!(
            state.settings[EXPRESSION],
            manifest::sha256_hex(b"expression 2")
        );
        assert_eq!(
            state.settings[CONTROLS],
            manifest::sha256_hex(b"controls 1")
        );
        // Applied again: nothing more to write.
        lamp.said.clear();
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert!(!lamp.said.iter().any(|line| line.starts_with("wrote")));
    }

    #[test]
    fn put_back_restores_each_file_byte_for_byte_and_its_mode() {
        let mut lamp = Lamp::new();
        lamp.installed(&[
            (EXPRESSION, "expression 1"),
            (TMPFILES, "d /run/a 0755 root root -\n"),
        ]);
        fs::set_permissions(
            on_root(&lamp.root, EXPRESSION),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        let before = lamp.state();
        let new_skill = "/var/lib/lampOS/skills/dance/SKILL.md";
        let envelope = lamp.envelope(
            2,
            vec![
                Setting::new(EXPRESSION, "expression 2"),
                Setting::new(TMPFILES, "d /run/b 0755 root root -\n"),
                Setting::new(new_skill, "dance"),
            ],
        );
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.tmpfiles, [TMPFILES]);
        assert_eq!(lamp.read(new_skill).as_deref(), Some("dance"));
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 1"));
        let mode = fs::metadata(on_root(&lamp.root, EXPRESSION))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
        assert_eq!(
            lamp.read(TMPFILES).as_deref(),
            Some("d /run/a 0755 root root -\n")
        );
        assert_eq!(lamp.read(new_skill), None, "one the build added is gone");
        assert_eq!(lamp.tmpfiles, [TMPFILES, TMPFILES]);
        assert_eq!(lamp.state(), before);
    }

    #[test]
    fn put_back_keeps_a_file_changed_since_and_a_stale_back_file_is_not_used() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1")]);
        let envelope = lamp.envelope(2, vec![Setting::new(EXPRESSION, "expression 2")]);
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        lamp.write(EXPRESSION, "the person's");
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("the person's"));
        // Build 3 is put back before its apply wrote anything: build 2's
        // back.json is not build 3's.
        lamp.write(EXPRESSION, "expression 2");
        lamp.run(Op::Back, 3, None).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 2"));
    }

    #[test]
    fn a_setting_the_lamp_removed_stays_removed_and_one_never_there_is_written() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1")]);
        fs::remove_file(on_root(&lamp.root, EXPRESSION)).unwrap();
        let records = "/etc/lampOS/records.json";
        let envelope = lamp.envelope(
            2,
            vec![
                Setting::new(EXPRESSION, "expression 2"),
                Setting::new(records, "{}"),
            ],
        );
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(EXPRESSION), None);
        assert_eq!(lamp.read(records).as_deref(), Some("{}"));
    }

    #[test]
    fn a_removed_setting_goes_only_where_it_is_still_the_builds() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1"), (CONTROLS, "controls 1")]);
        lamp.write(CONTROLS, "the person's");
        let gone = |path: &str| Setting {
            path: path.into(),
            text: None,
            sha256: None,
            mode: None,
            removed: true,
        };
        let envelope = lamp.envelope(2, vec![gone(EXPRESSION), gone(CONTROLS)]);
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(EXPRESSION), None);
        assert_eq!(lamp.read(CONTROLS).as_deref(), Some("the person's"));
        assert!(!lamp.state().settings.contains_key(EXPRESSION));
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 1"));
    }

    #[test]
    fn an_envelope_signed_with_another_key_or_of_another_build_writes_nothing() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1")]);
        let mut other = Lamp::new();
        let forged = other.envelope(2, vec![Setting::new(EXPRESSION, "forged")]);
        assert!(lamp.run(Op::Apply, 2, Some(&forged)).is_err());
        let envelope = lamp.envelope(2, vec![Setting::new(EXPRESSION, "expression 2")]);
        assert!(lamp.run(Op::Apply, 3, Some(&envelope)).is_err());
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 1"));
        assert!(!lamp.records.join(FOLDER).join(BACK).exists());
        other.said.clear();
    }

    /// The reviewer on #1913, 19:50: a put-back cut short after restoring a
    /// file, asked again, keeps that file's record put back.
    #[test]
    fn a_put_back_asked_again_after_being_cut_short_puts_every_record_back() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1"), (CONTROLS, "controls 1")]);
        let before = lamp.state();
        let envelope = lamp.envelope(
            2,
            vec![
                Setting::new(EXPRESSION, "expression 2"),
                Setting::new(CONTROLS, "controls 2"),
            ],
        );
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        // The first put-back restored the expression, then was cut short.
        lamp.write(EXPRESSION, "expression 1");
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.read(EXPRESSION).as_deref(), Some("expression 1"));
        assert_eq!(lamp.read(CONTROLS).as_deref(), Some("controls 1"));
        assert_eq!(lamp.state(), before);
    }

    #[test]
    fn a_new_skills_folder_is_removed_when_it_is_put_back_and_a_folder_in_use_stays() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(EXPRESSION, "expression 1")]);
        let (dance, sing) = (
            "/var/lib/lampOS/skills/dance/SKILL.md",
            "/var/lib/lampOS/skills/sing/SKILL.md",
        );
        let envelope = lamp.envelope(
            2,
            vec![Setting::new(dance, "dance"), Setting::new(sing, "sing")],
        );
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        // The person added a file of their own beside the new sing skill.
        lamp.write("/var/lib/lampOS/skills/sing/notes.md", "mine");
        lamp.run(Op::Back, 2, None).unwrap();
        assert!(!on_root(&lamp.root, "/var/lib/lampOS/skills/dance").exists());
        assert_eq!(lamp.read(sing), None);
        assert_eq!(
            lamp.read("/var/lib/lampOS/skills/sing/notes.md").as_deref(),
            Some("mine")
        );
    }

    #[test]
    fn the_tmpfiles_are_created_only_when_they_are_there() {
        let mut lamp = Lamp::new();
        lamp.installed(&[(TMPFILES, "d /run/a 0755 root root -\n")]);
        let gone = Setting {
            path: TMPFILES.into(),
            text: None,
            sha256: None,
            mode: None,
            removed: true,
        };
        let envelope = lamp.envelope(2, vec![gone]);
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(TMPFILES), None);
        assert!(lamp.tmpfiles.is_empty(), "{:?}", lamp.tmpfiles);
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.tmpfiles, [TMPFILES]);
        // A build that adds them, put back: removed, and nothing to create.
        let mut lamp = Lamp::new();
        lamp.installed(&[]);
        let envelope = lamp.envelope(
            3,
            vec![Setting::new(TMPFILES, "d /run/b 0755 root root -\n")],
        );
        lamp.run(Op::Apply, 3, Some(&envelope)).unwrap();
        lamp.run(Op::Back, 3, None).unwrap();
        assert_eq!(lamp.read(TMPFILES), None);
        assert_eq!(lamp.tmpfiles, [TMPFILES]);
    }

    /// The reviewer on #1913, 19:50: an install before #1680 wrote the
    /// tmpfiles without a record. One install shipped is the build's
    /// before; any other is the lamp's own.
    #[test]
    fn unrecorded_tmpfiles_an_install_shipped_are_the_builds_and_others_the_lamps() {
        for (there, written) in [
            (crate::install::TMPFILES, true),
            ("d /run/mine 0755 root root -\n", false),
        ] {
            let mut lamp = Lamp::new();
            lamp.installed(&[]);
            lamp.write(TMPFILES, there);
            let new = "d /run/new 0755 root root -\n";
            let envelope = lamp.envelope(2, vec![Setting::new(TMPFILES, new)]);
            lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
            let expected = if written { new } else { there };
            assert_eq!(lamp.read(TMPFILES).as_deref(), Some(expected), "{there:?}");
        }
    }

    #[test]
    fn the_tmpfiles_install_writes_is_one_it_shipped() {
        let sha = manifest::sha256_hex(crate::install::TMPFILES.as_bytes());
        assert!(
            TMPFILES_SHIPPED.contains(&sha.as_str()),
            "add {sha} to TMPFILES_SHIPPED"
        );
    }

    #[test]
    fn without_a_request_nothing_is_done() {
        let lamp = Lamp::new();
        let key: [u8; 32] = lamp.key.public_key().as_ref().try_into().unwrap();
        let mut said = Vec::new();
        run(
            &lamp.root,
            &lamp.records,
            &key,
            &mut |_| panic!("no tmpfiles"),
            &mut |line| said.push(line.to_owned()),
        )
        .unwrap();
        assert_eq!(said, ["nothing is asked: no request"]);
    }

    /// The request and back.json are read by a lamp-update-apply as old as
    /// the lamp's install: a field it does not know is ignored.
    #[test]
    fn a_request_with_a_field_it_does_not_know_is_read() {
        let request: Request =
            serde_json::from_str(r#"{"version":2,"op":"back","sequence":4,"later":true}"#).unwrap();
        assert_eq!(request.op, Op::Back);
        let back: Back = serde_json::from_str(
            r#"{"version":2,"sequence":4,"files":[{"path":"/etc/lampOS/records.json","wrote":null,"owner":"x"}],"more":1}"#,
        )
        .unwrap();
        assert_eq!(back.files[0].bytes, None);
    }
    const SPEAKER: &str = "/etc/systemd/system/lamp-cmedia-speaker.service";
    const RULES: &str = "/etc/udev/rules.d/99-lamp-device.rules";
    const ASOUND: &str = "/etc/asound.conf";
    const JOURNAL: &str = "/etc/systemd/journald.conf.d/zz-lamp-journal.conf";

    fn image_file(variant: &str, path: &str, text: &str, shipped: &[&str]) -> ImageFile {
        ImageFile {
            variant: variant.into(),
            path: path.into(),
            text: text.into(),
            sha256: manifest::sha256_hex(text.as_bytes()),
            shipped: shipped
                .iter()
                .map(|text| manifest::sha256_hex(text.as_bytes()))
                .collect(),
        }
    }

    /// A lamp flashed with an earlier image, one file of it changed by hand.
    fn flashed(lamp: &Lamp) {
        lamp.installed(&[]);
        lamp.write(SPEAKER, "speaker 9, remain");
        lamp.write(RULES, "rules of this lamp");
        lamp.write(ASOUND, "asound 1");
        lamp.write(JOURNAL, "journal 1");
        // The person's drop-in beside the unit.
        lamp.write(
            "/etc/systemd/system/lamp-cmedia-speaker.service.d/30-local-gain.conf",
            "local gain",
        );
    }

    fn image_build(lamp: &Lamp, variant_text: &str) -> Vec<u8> {
        lamp.envelope_with(
            2,
            Vec::new(),
            vec![
                image_file(
                    "standard",
                    SPEAKER,
                    "speaker 9",
                    &["speaker 9, remain", "speaker 9"],
                ),
                image_file("pro", SPEAKER, variant_text, &["speaker 6, remain"]),
                image_file("standard", RULES, "rules 2", &["rules 1", "rules 2"]),
                image_file("standard", ASOUND, "asound 2", &["asound 1", "asound 2"]),
                image_file(
                    "standard",
                    JOURNAL,
                    "journal 2",
                    &["journal 1", "journal 2"],
                ),
            ],
        )
    }

    #[test]
    fn the_images_files_are_written_where_the_lamp_has_one_the_image_shipped() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        let envelope = image_build(&lamp, "speaker 6");
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(SPEAKER).as_deref(), Some("speaker 9"));
        assert_eq!(lamp.read(ASOUND).as_deref(), Some("asound 2"));
        assert_eq!(lamp.read(JOURNAL).as_deref(), Some("journal 2"));
        // Changed by hand: none the image shipped, kept and said.
        assert_eq!(lamp.read(RULES).as_deref(), Some("rules of this lamp"));
        assert!(
            lamp.said
                .iter()
                .any(|line| line.starts_with(&format!("kept {RULES}"))),
            "{:?}",
            lamp.said
        );
        // A drop-in is never the image's.
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-cmedia-speaker.service.d/30-local-gain.conf")
                .as_deref(),
            Some("local gain")
        );
        assert_eq!(
            lamp.commands,
            [
                "systemd-analyze verify /run/lampos-update-apply/lamp-cmedia-speaker.service",
                "systemctl daemon-reload",
                "systemctl start lamp-cmedia-speaker.service"
            ]
        );
        assert!(!lamp.commands.iter().any(|c| c.contains("journald")));
        assert!(
            !lamp
                .root
                .join("run/lampos-update-apply/lamp-cmedia-speaker.service")
                .exists()
        );
        assert!(
            lamp.said.iter().any(|line| line.contains("next boot")),
            "{:?}",
            lamp.said
        );
        let state = lamp.state();
        assert_eq!(state.settings[SPEAKER], manifest::sha256_hex(b"speaker 9"));
        assert!(!state.settings.contains_key(RULES));
        // Applied again: nothing more.
        lamp.commands.clear();
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert!(lamp.commands.is_empty(), "{:?}", lamp.commands);
    }

    #[test]
    fn the_variants_file_is_written_and_none_of_another() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        lamp.installed(&[]);
        lamp.write(image::PROFILE, "pro\n");
        lamp.write(SPEAKER, "speaker 6, remain");
        lamp.write(RULES, "rules 1");
        let envelope = image_build(&lamp, "speaker 6");
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(SPEAKER).as_deref(), Some("speaker 6"));
        // The build has no pro rules: the lamp's stay as they are.
        assert_eq!(lamp.read(RULES).as_deref(), Some("rules 1"));
        assert_eq!(lamp.read(ASOUND), None);
    }

    #[test]
    fn put_back_restores_the_images_files_and_reloads_again() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        lamp.write(RULES, "rules 1");
        let envelope = image_build(&lamp, "speaker 6");
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(RULES).as_deref(), Some("rules 2"));
        lamp.commands.clear();
        lamp.run(Op::Back, 2, None).unwrap();
        assert_eq!(lamp.read(SPEAKER).as_deref(), Some("speaker 9, remain"));
        assert_eq!(lamp.read(RULES).as_deref(), Some("rules 1"));
        assert_eq!(lamp.read(ASOUND).as_deref(), Some("asound 1"));
        assert_eq!(
            lamp.commands,
            [
                "systemctl daemon-reload",
                "udevadm control --reload",
                "systemctl start lamp-cmedia-speaker.service"
            ]
        );
        assert!(lamp.state().settings.is_empty());
    }

    /// The reviewer on #1945, 21:43: a signed build whose image names a
    /// path outside `IMAGE_FILES` writes nothing, as root or otherwise.
    #[test]
    fn a_signed_image_file_outside_the_list_is_refused_with_nothing_written() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        let sudoers = "/etc/sudoers.d/lamp";
        let envelope = lamp.envelope_with(
            2,
            Vec::new(),
            vec![
                image_file("standard", ASOUND, "asound 2", &["asound 1"]),
                image_file("standard", sudoers, "lampos ALL=(ALL) NOPASSWD: ALL", &[]),
            ],
        );
        assert!(lamp.run(Op::Apply, 2, Some(&envelope)).is_err());
        assert_eq!(lamp.read(sudoers), None);
        assert_eq!(lamp.read(ASOUND).as_deref(), Some("asound 1"));
        assert!(!lamp.records.join(FOLDER).join(BACK).exists());
        assert!(lamp.commands.is_empty(), "{:?}", lamp.commands);
    }

    #[test]
    fn a_path_both_a_setting_and_an_image_file_is_refused_with_nothing_written() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        let envelope = lamp.envelope_with(
            2,
            vec![
                Setting::new(EXPRESSION, "expression 2"),
                Setting::new(ASOUND, "asound by settings"),
            ],
            vec![image_file("standard", ASOUND, "asound 2", &["asound 1"])],
        );
        assert!(lamp.run(Op::Apply, 2, Some(&envelope)).is_err());
        assert_eq!(lamp.read(ASOUND).as_deref(), Some("asound 1"));
        assert_eq!(lamp.read(EXPRESSION), None);
        assert!(!lamp.records.join(FOLDER).join(BACK).exists());
        assert!(lamp.commands.is_empty(), "{:?}", lamp.commands);
    }

    #[test]
    fn without_the_image_asked_or_from_an_older_copy_none_is_written() {
        let mut lamp = Lamp::new();
        flashed(&lamp);
        let envelope = image_build(&lamp, "speaker 6");
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert_eq!(lamp.read(SPEAKER).as_deref(), Some("speaker 9, remain"));
        assert!(lamp.commands.is_empty());
        // A copy older than the field reads the manifest as before.
        let bytes = manifest::signed(
            &envelope,
            &lamp.key.public_key().as_ref().try_into().unwrap(),
        )
        .unwrap();
        #[derive(Deserialize)]
        struct Older {
            sequence: u64,
            #[serde(default)]
            settings: Vec<Setting>,
        }
        let older: Older = serde_json::from_slice(&bytes).unwrap();
        assert_eq!((older.sequence, older.settings.len()), (2, 0));
    }

    #[test]
    fn a_reload_that_fails_fails_the_apply_and_a_speaker_that_fails_is_said() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        lamp.failing = Some("systemctl start".into());
        let envelope = image_build(&lamp, "speaker 6");
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        assert!(
            lamp.said
                .iter()
                .any(|line| line.starts_with("systemctl start")),
            "{:?}",
            lamp.said
        );
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        lamp.failing = Some("systemctl daemon-reload".into());
        let envelope = image_build(&lamp, "speaker 6");
        assert!(lamp.run(Op::Apply, 2, Some(&envelope)).is_err());
    }

    /// A unit systemd can't load is never written: a broken unit at boot.
    #[test]
    fn a_unit_systemd_cannot_load_writes_nothing() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        flashed(&lamp);
        lamp.failing = Some("systemd-analyze verify".into());
        let envelope = image_build(&lamp, "speaker 6");
        let why = lamp.run(Op::Apply, 2, Some(&envelope)).unwrap_err();
        assert!(why.contains("systemd can't load it"), "{why}");
        assert_eq!(lamp.read(SPEAKER).as_deref(), Some("speaker 9, remain"));
        assert_eq!(lamp.read(ASOUND).as_deref(), Some("asound 1"));
        assert!(!lamp.records.join(FOLDER).join(BACK).exists());
    }

    /// The first builder on #1680, 19:48: an apply and its put-back of
    /// every one of the image's files, journald's drop-in among them, run
    /// nothing of journald; and the guard every command passes refuses one.
    #[test]
    fn the_apply_path_never_restarts_or_signals_journald() {
        let mut lamp = Lamp::new();
        lamp.image = true;
        lamp.installed(&[]);
        let mut image = Vec::new();
        for path in manifest::IMAGE_FILES {
            lamp.write(path, &format!("{path} 1"));
            image.push(image_file(
                "standard",
                path,
                &format!("{path} 2"),
                &[&format!("{path} 1")],
            ));
        }
        let envelope = lamp.envelope_with(2, Vec::new(), image);
        lamp.run(Op::Apply, 2, Some(&envelope)).unwrap();
        lamp.run(Op::Back, 2, None).unwrap();
        assert!(!lamp.commands.is_empty());
        for command in &lamp.commands {
            assert!(!command.contains("journald"), "{command}");
            let parts: Vec<&str> = command.split(' ').collect();
            image::guard(&parts).unwrap();
        }
        for refused in [
            &["systemctl", "restart", "systemd-journald.service"][..],
            &["systemctl", "kill", "--signal=HUP", "systemd-journald"],
        ] {
            assert!(image::guard(refused).is_err(), "{refused:?}");
        }
    }
    /// lamp-update-apply.service can write every path a build may name,
    /// and names nothing of journald.
    #[test]
    fn the_apply_unit_can_write_every_carried_path_and_never_names_journald() {
        let unit = include_str!("../../../deploy/product/lamp-update-apply.service");
        let writes: Vec<&str> = unit
            .lines()
            .find_map(|line| line.strip_prefix("ReadWritePaths="))
            .unwrap()
            .split_whitespace()
            .collect();
        let writable = |path: &str| {
            let folder = path.rsplit_once('/').unwrap().0;
            writes
                .iter()
                .any(|at| folder == *at || folder.starts_with(&format!("{at}/")))
        };
        for path in manifest::IMAGE_FILES
            .iter()
            .chain(manifest::SETTING_FILES)
            .chain([&manifest::RESET_PLAN])
            .copied()
            .chain(
                manifest::SETTING_FOLDERS
                    .iter()
                    .map(|f| f.trim_end_matches('/')),
            )
        {
            assert!(writable(path), "{path}: {writes:?}");
        }
        assert!(
            unit.lines()
                .filter(|line| !line.starts_with('#'))
                .all(|line| !line.contains("journald"))
        );
        assert!(unit.contains("ReadOnlyPaths=-/etc/lampOS/update"));
    }
}
