//! Captured ordinary setting ownership, for a future writer guard (#3689).
//!
//! **Unwired and raw:** no authentication, projection, grant, persistence or
//! effects are performed here. These are not an executable apply plan. The
//! caller supplies the observer's trusted stable root and excludes writers.
//! Ordinary `apply::plan` retains its existing filesystem behavior.
use crate::{
    apply::{self, Act},
    install, manifest,
    manifest::Setting,
    writer_files::{self, Entry, Observation},
};
use std::{collections::BTreeMap, fmt, io, path::Path};

/// Independent of the signed new-text limit: old files may be larger.
pub const MOST_OLD_BYTES: usize = 4 * 1024 * 1024;

/// One input and its exact observation, including an omitted ordinary no-op.
/// Borrowing prevents the input setting/selected record values being changed
/// through these references while the capture exists. It is not disk exclusion.
pub struct Captured<'a> {
    setting: &'a Setting,
    recorded: Option<&'a str>,
    writing: Option<&'a str>,
    before: Observation,
    act: Option<Act>,
}

impl<'a> Captured<'a> {
    pub fn setting(&self) -> &'a Setting {
        self.setting
    }

    pub fn recorded(&self) -> Option<&'a str> {
        self.recorded
    }

    pub fn writing(&self) -> Option<&'a str> {
        self.writing
    }

    pub fn before(&self) -> &Observation {
        &self.before
    }

    /// Ordinary ownership without an image-shipped before map. This decision
    /// is not image-aware and is not an executable apply-plan admission.
    pub fn act(&self) -> Option<&Act> {
        self.act.as_ref()
    }
}

impl fmt::Debug for Captured<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Captured")
            .field("before", &self.before)
            .field("has_action", &self.act.is_some())
            .finish_non_exhaustive()
    }
}

pub struct RawOwnership<'a> {
    entries: Vec<Captured<'a>>,
    old_bytes: usize,
}

impl<'a> RawOwnership<'a> {
    pub fn entries(&self) -> &[Captured<'a>] {
        &self.entries
    }

    pub fn old_bytes(&self) -> usize {
        self.old_bytes
    }

    /// Re-observe the captured paths and require the same bytes, entry kinds,
    /// selected metadata and ancestor identities. Each read is capped at its
    /// original byte count, so growth cannot increase the capture's budget.
    ///
    /// This read-only check grants no authority and retains no live descriptors.
    /// The caller must keep the trusted root and writer exclusion in force
    /// through any later effect. It is not an atomic cross-file snapshot. An
    /// empty capture observes nothing and establishes no root admission.
    pub fn revalidate(&self, root: &Path) -> io::Result<()> {
        for captured in &self.entries {
            let relative = captured
                .setting
                .path
                .strip_prefix('/')
                .ok_or_else(|| invalid("captured setting is not absolute"))?;
            let cap = match &captured.before.entry {
                Entry::Present { bytes, .. } => bytes.len(),
                Entry::MissingLeaf | Entry::MissingAncestor { .. } => 0,
            };
            let current = writer_files::observe(root, relative, cap)?;
            if !same_observation(&captured.before, &current) {
                return Err(invalid("captured setting observation changed"));
            }
        }
        Ok(())
    }
}

fn same_observation(before: &Observation, current: &Observation) -> bool {
    before.ancestors == current.ancestors
        && match (&before.entry, &current.entry) {
            (
                Entry::Present {
                    bytes: before_bytes,
                    identity: before_identity,
                },
                Entry::Present {
                    bytes: current_bytes,
                    identity: current_identity,
                },
            ) => before_identity == current_identity && before_bytes == current_bytes,
            (Entry::MissingLeaf, Entry::MissingLeaf) => true,
            (
                Entry::MissingAncestor { component: before },
                Entry::MissingAncestor { component: current },
            ) => before == current,
            _ => false,
        }
}

impl fmt::Debug for RawOwnership<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawOwnership")
            .field("entry_count", &self.entries.len())
            .field("old_bytes", &self.old_bytes)
            .finish()
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn record(value: Option<&String>, marker: bool) -> io::Result<Option<&str>> {
    match value {
        None => Ok(None),
        Some(value)
            if value.len() <= 64 && (digest(value) || (marker && value == install::SET_ASIDE)) =>
        {
            Ok(Some(value.as_str()))
        }
        Some(_) => Err(invalid("unsupported captured setting record")),
    }
}

// Bound caller-constructed values before hashing or formatting validation
// errors. Only selected map entries are inspected; unrelated state is not read,
// copied, defaulted or replaced. The ordinary planner's input policy is unchanged.
fn preflight(
    settings: &[Setting],
    recorded: &BTreeMap<String, String>,
    writing: &BTreeMap<String, String>,
) -> io::Result<()> {
    if settings.len() > manifest::MOST_SETTINGS {
        return Err(invalid("too many captured settings"));
    }
    let mut text_bytes = 0usize;
    for setting in settings {
        if setting.path.len() >= 512
            || setting
                .path
                .strip_prefix('/')
                .is_none_or(|relative| relative.split('/').count() > 64)
            || setting.sha256.as_ref().is_some_and(|sha| sha.len() != 64)
            || setting.mode.as_ref().is_some_and(|mode| mode.len() != 4)
        {
            return Err(invalid("invalid captured setting fields"));
        }
        text_bytes = text_bytes
            .checked_add(setting.text.as_ref().map_or(0, String::len))
            .filter(|count| *count <= manifest::MOST_SETTINGS_BYTES)
            .ok_or_else(|| invalid("captured new settings exceed their byte limit"))?;
        record(recorded.get(&setting.path), true)?;
        record(writing.get(&setting.path), false)?;
    }
    manifest::check_settings(settings).map_err(|_| invalid("invalid captured settings"))
}

/// Capture every ordinary input, in order, using only the returned file bytes
/// for ownership. Missing ancestors and missing leaves classify as missing,
/// while their distinct before-images remain retained. Total old bytes are
/// bounded independently of new signed text; failure returns no partial capture.
///
/// No image/reset selection or head-touch projection runs here. A changed
/// tmpfiles/skill/controls file is not thereby approved for execution or undo.
/// Per-file checks and matching observed root identities are not an atomic
/// cross-file snapshot; final-boundary revalidation remains future work.
pub fn capture<'a>(
    root: &Path,
    settings: &'a [Setting],
    recorded: &'a BTreeMap<String, String>,
    writing: &'a BTreeMap<String, String>,
) -> io::Result<RawOwnership<'a>> {
    preflight(settings, recorded, writing)?;
    let mut entries = Vec::with_capacity(settings.len());
    let mut old_bytes = 0usize;
    let mut root_identity = None;
    let shipped = BTreeMap::new();
    for setting in settings {
        // check_settings already admitted one exact absolute compiled path.
        let relative = setting
            .path
            .strip_prefix('/')
            .ok_or_else(|| invalid("captured setting is not absolute"))?;
        let cap = writer_files::MOST_BYTES.min(MOST_OLD_BYTES - old_bytes);
        let before = writer_files::observe(root, relative, cap)?;
        let identity = before
            .ancestors
            .first()
            .copied()
            .ok_or_else(|| invalid("captured setting lacks its root identity"))?;
        if root_identity.is_some_and(|prior| prior != identity) {
            return Err(invalid("captured settings root changed"));
        }
        root_identity = Some(identity);
        let now = match &before.entry {
            Entry::Present { bytes, .. } => {
                old_bytes = old_bytes
                    .checked_add(bytes.len())
                    .filter(|count| *count <= MOST_OLD_BYTES)
                    .ok_or_else(|| invalid("captured old settings exceed their byte limit"))?;
                Some(manifest::sha256_hex(bytes))
            }
            Entry::MissingLeaf | Entry::MissingAncestor { .. } => None,
        };
        let act = apply::observed_act(setting, now.as_deref(), recorded, writing, &shipped);
        entries.push(Captured {
            setting,
            recorded: record(recorded.get(&setting.path), true)?,
            writing: record(writing.get(&setting.path), false)?,
            before,
            act,
        });
    }
    Ok(RawOwnership { entries, old_bytes })
}

#[cfg(test)]
mod tests;
