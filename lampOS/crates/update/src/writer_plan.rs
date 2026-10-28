//! Pure, bounded ordinary-settings preparation for a future writer (#3689).
//!
//! The protected context's provenance, root ancestry, excluded writers and
//! durable admission remain the caller's responsibility. This module reads no
//! filesystem or State, grants nothing and performs no effect or publication.
use crate::{
    apply::{self, Act, Step},
    head_touch::{self, capture::codec::SavedInputs},
    install,
    manifest::{self, Manifest, Setting},
    writer_files::{Entry, Identity, Observation},
    writer_preimages::{Bundle, MOST_BUNDLE_BYTES},
    writer_refresh::{Artifact, Binding},
};
use std::{collections::BTreeMap, fmt, os::unix::ffi::OsStrExt, path::Path};

pub(crate) const ORDINARY_DERIVATION: u32 = 1;

pub(crate) mod saved;
const MOST_CONTEXT_PATH: usize = 4096;
const MOST_COMPONENTS: usize = 64;
// Conservative preparation bounds mirror the existing codec/projector. A
// future expansion of either producer does not silently widen this derivation.
const MOST_HEAD_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const MOST_PINS_BYTES: usize = 1024 * 1024;
const REFUSED: &str = "ordinary plan preparation refused";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refused;

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REFUSED)
    }
}

impl fmt::Debug for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REFUSED)
    }
}

impl std::error::Error for Refused {}

pub(crate) struct SelectedRecord<'a> {
    path: &'a str,
    recorded: Option<&'a str>,
    writing: Option<&'a str>,
}

/// Independently selected from protected product-before context, not Bundle.
/// Constructing this value establishes neither authentication nor provenance.
pub(crate) struct ProductSelection<'a> {
    artifact: &'a Artifact,
    for_sequence: u64,
    installed_sequence: Option<u64>,
    refused: bool,
    records: &'a [SelectedRecord<'a>],
}

pub(crate) struct ExpectedOrdinary<'a> {
    transaction: &'a str,
    envelope: &'a Artifact,
    channel: &'a str,
    sequence: u64,
    commit: &'a str,
    trusted_key_sha256: [u8; 32],
    refresh: &'a Binding,
    product: ProductSelection<'a>,
    preimages: &'a Artifact,
    head_inputs: &'a Artifact,
    root: &'a Path,
    installed_units: &'a Path,
    ordinary_enabled: bool,
    derivation: u32,
}

struct Row {
    signed_index: usize,
    initial: Option<Act>,
    effective: Option<Step>,
}

pub(crate) struct PreparedOrdinary<'a> {
    _expected: &'a ExpectedOrdinary<'a>,
    envelope: Box<[u8]>,
    signed_bytes: Vec<u8>,
    manifest: Manifest,
    preimages: Bundle,
    head_inputs: SavedInputs,
    rows: Vec<Row>,
    projected_order: Vec<usize>,
    has_content_changes: bool,
}

impl fmt::Debug for PreparedOrdinary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedOrdinary")
            .field("envelope_bytes", &self.envelope.len())
            .field("signed_bytes", &self.signed_bytes.len())
            .field("settings", &self.manifest.settings.len())
            .field("preimage_bytes", &self.preimages.encoded().len())
            .field("head_input_bytes", &self.head_inputs.as_bytes().len())
            .field("rows", &self.rows.len())
            .field("projected_steps", &self.projected_order.len())
            .field("has_content_changes", &self.has_content_changes)
            .finish()
    }
}

fn require(condition: bool) -> Result<(), Refused> {
    if condition { Ok(()) } else { Err(Refused) }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn known_artifact(artifact: &Artifact, most: u64) -> bool {
    hex(&artifact.sha256, 64) && artifact.size != 0 && artifact.size <= most
}

fn canonical(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.len() > MOST_CONTEXT_PATH || bytes[0] != b'/' || bytes.contains(&0)
    {
        return false;
    }
    if bytes == b"/" {
        return true;
    }
    let mut count = 0;
    bytes[1..].split(|b| *b == b'/').all(|part| {
        count += 1;
        count <= MOST_COMPONENTS && !part.is_empty() && !matches!(part, b"." | b"..")
    })
}

fn rooted_units(root: &Path, units: &Path) -> bool {
    if !canonical(root) || !canonical(units) {
        return false;
    }
    let root = root.as_os_str().as_bytes();
    let units = units.as_os_str().as_bytes();
    if root == b"/" {
        units.len() > 1
    } else {
        units
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with(b"/") && tail.len() > 1)
    }
}

// All known borrowed caller fields are bounded before copying or invoking
// validators that might format a diagnostic. Unknown Binding/Later data is
// never cloned, serialized or included in an error/Debug representation.
fn check_expected(expected: &ExpectedOrdinary<'_>) -> Result<(), Refused> {
    require(
        expected.ordinary_enabled
            && expected.derivation == ORDINARY_DERIVATION
            && hex(expected.transaction, 64)
            && manifest::is_token(expected.channel)
            && hex(expected.commit, 40)
            && expected.sequence != 0
            && hex(&expected.refresh.transaction, 64)
            && expected.product.records.len() <= manifest::MOST_SETTINGS
            && rooted_units(expected.root, expected.installed_units),
    )?;
    for (artifact, most) in [
        (expected.envelope, manifest::MOST_ENVELOPE_BYTES),
        (expected.preimages, MOST_BUNDLE_BYTES as u64),
        (expected.head_inputs, MOST_HEAD_INPUT_BYTES),
        (expected.product.artifact, manifest::MOST_FILE_BYTES),
        (&expected.refresh.envelope, manifest::MOST_ENVELOPE_BYTES),
        (&expected.refresh.guard, manifest::MOST_FILE_BYTES),
        (&expected.refresh.writer, manifest::MOST_FILE_BYTES),
        (&expected.refresh.previous_writer, manifest::MOST_FILE_BYTES),
        (&expected.refresh.product_before, manifest::MOST_FILE_BYTES),
    ] {
        require(known_artifact(artifact, most))?;
    }
    for record in expected.product.records {
        require(
            record.path.len() < 512
                && record
                    .path
                    .strip_prefix('/')
                    .is_some_and(|path| path.split('/').count() <= MOST_COMPONENTS)
                && manifest::is_setting_path(record.path)
                && record
                    .recorded
                    .is_none_or(|value| hex(value, 64) || value == install::SET_ASIDE)
                && record.writing.is_none_or(|value| hex(value, 64)),
        )?;
    }
    expected.refresh.check().map_err(|_| Refused)?;
    for (artifact, most) in [
        (expected.envelope, manifest::MOST_ENVELOPE_BYTES),
        (expected.preimages, MOST_BUNDLE_BYTES as u64),
        (expected.head_inputs, MOST_HEAD_INPUT_BYTES),
        (expected.product.artifact, manifest::MOST_FILE_BYTES),
    ] {
        artifact.check(most).map_err(|_| Refused)?;
    }
    require(
        expected.transaction != expected.refresh.transaction
            && expected.sequence >= expected.refresh.sequence
            && (expected.sequence != expected.refresh.sequence
                || expected.envelope.matches(&expected.refresh.envelope))
            && expected.product.for_sequence == expected.sequence
            && !expected.product.refused
            && expected
                .product
                .installed_sequence
                .is_none_or(|installed| expected.sequence > installed)
            && expected
                .product
                .artifact
                .matches(&expected.refresh.product_before),
    )
}

fn supported(manifest: &Manifest, writer: &Artifact) -> Result<(), Refused> {
    require(
        manifest.files.len() == 1
            && manifest.files[0].name == "lamp-update"
            && manifest.files[0].sha256 == writer.sha256
            && manifest.files[0].size == writer.size
            && manifest.units.is_empty()
            && manifest.models.is_empty()
            && manifest.image.is_empty()
            && manifest.reset_plan.is_none()
            && !manifest.settings.is_empty()
            && manifest.settings.iter().all(|setting| {
                setting.path != install::TMPFILES_PATH
                    && !setting.path.starts_with("/var/lib/lampOS/skills/")
                    && setting.path != manifest::RESET_PLAN
            }),
    )
}

enum Prefix<'a> {
    Directory(&'a Identity),
    Regular(&'a Identity, &'a [u8]),
    Absent,
}

fn prefix(observed: &Observation, parts: usize, index: usize) -> Result<Prefix<'_>, Refused> {
    if let Some(identity) = observed.ancestors.get(index) {
        return Ok(Prefix::Directory(identity));
    }
    match &observed.entry {
        Entry::Present { bytes, identity } if index == parts => {
            Ok(Prefix::Regular(identity, bytes))
        }
        Entry::MissingLeaf if index == parts => Ok(Prefix::Absent),
        Entry::MissingAncestor { component } if index > *component => Ok(Prefix::Absent),
        _ => Err(Refused),
    }
}

// Both decoders accept individually shaped rows, not a simultaneous snapshot.
// Compare every represented common lexical prefix between ordinary rows too;
// absent Head keys must not hide a contradictory ordinary parent or leaf.
fn coherent_bundle(bundle: &Bundle) -> Result<(), Refused> {
    require(bundle.entries().len() <= manifest::MOST_SETTINGS)?;
    let paths: Vec<Vec<&str>> = bundle
        .entries()
        .iter()
        .map(|entry| {
            entry
                .path()
                .strip_prefix('/')
                .ok_or(Refused)
                .map(|path| path.split('/').collect())
        })
        .collect::<Result<_, _>>()?;
    for parts in &paths {
        require(parts.len() <= MOST_COMPONENTS)?;
    }
    for (left, left_parts) in paths.iter().enumerate() {
        for (right, right_parts) in paths.iter().enumerate().skip(left + 1) {
            let common = left_parts
                .iter()
                .zip(right_parts)
                .take_while(|(a, b)| a == b)
                .count();
            for index in 0..=common {
                let a = prefix(bundle.entries()[left].before(), left_parts.len(), index)?;
                let b = prefix(bundle.entries()[right].before(), right_parts.len(), index)?;
                require(match (a, b) {
                    (Prefix::Directory(a), Prefix::Directory(b)) => a == b,
                    (Prefix::Regular(a, bytes_a), Prefix::Regular(b, bytes_b)) => {
                        a == b && bytes_a == bytes_b
                    }
                    (Prefix::Absent, Prefix::Absent) => true,
                    _ => false,
                })?;
            }
        }
    }
    Ok(())
}

fn reconciles_pins(signed: &Setting, initial: Option<&Act>, effective: &Step) -> bool {
    if effective.setting.path != signed.path
        || effective.setting.mode != signed.mode
        || effective.setting.removed != signed.removed
    {
        return false;
    }
    match initial {
        Some(act @ (Act::Keep(_) | Act::Remove)) => {
            effective.setting == *signed && effective.act == *act
        }
        Some(Act::Write | Act::Record) | None => {
            // Only the trusted capped projector produces this vector. Its
            // policy owns text derivation; here only text/SHA/action may vary.
            // Do not add another JSON parser or duplicate the head policy.
            let (Some(text), Some(sha)) = (&effective.setting.text, &effective.setting.sha256)
            else {
                return false;
            };
            matches!(effective.act, Act::Write | Act::Record)
                && text.len() <= MOST_PINS_BYTES
                && hex(sha, 64)
                && manifest::sha256_hex(text.as_bytes()) == *sha
                && (initial.is_some()
                    || effective.setting.text != signed.text
                    || effective.setting.sha256 != signed.sha256)
        }
    }
}

fn reconcile(
    settings: &[Setting],
    rows: &mut [Row],
    projected: Vec<Step>,
) -> Result<Vec<usize>, Refused> {
    require(
        settings.len() <= manifest::MOST_SETTINGS
            && settings.len() == rows.len()
            && projected.len() <= settings.len(),
    )?;
    require(
        rows.iter()
            .enumerate()
            .all(|(index, row)| row.signed_index == index && row.effective.is_none()),
    )?;
    let mut effective = vec![None; settings.len()];
    let mut order = Vec::with_capacity(projected.len());
    for step in projected {
        let index = settings
            .iter()
            .position(|setting| setting.path == step.setting.path)
            .ok_or(Refused)?;
        require(effective[index].is_none())?;
        let signed = &settings[index];
        let initial = rows[index].initial.as_ref();
        require(if signed.path == head_touch::PINS {
            reconciles_pins(signed, initial, &step)
        } else {
            step.setting == *signed && initial == Some(&step.act)
        })?;
        order.push(index);
        effective[index] = Some(step);
    }
    require(
        rows.iter()
            .zip(&effective)
            .all(|(row, step)| row.initial.is_none() || step.is_some()),
    )?;
    // Nothing is committed into rows until all reconciliation checks succeed.
    for (row, step) in rows.iter_mut().zip(effective) {
        row.effective = step;
    }
    Ok(order)
}

/// Prepare from authenticated-parent expectations and immutable decoded facts.
/// Hash/signature agreement does not authenticate the expected context, admit
/// root ancestry, establish freshness/exclusion or acknowledge FirstApply.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Private future writer preparation; no runtime caller is wired"
    )
)]
pub(crate) fn prepare<'a>(
    expected: &'a ExpectedOrdinary<'a>,
    envelope: &[u8],
    trusted_key: &[u8; 32],
    offered_writer_bytes: &[u8],
    preimages: Bundle,
    mut head_inputs: SavedInputs,
) -> Result<PreparedOrdinary<'a>, Refused> {
    check_expected(expected)?;
    require(
        envelope.len() as u64 == expected.envelope.size
            && envelope.len() as u64 <= manifest::MOST_ENVELOPE_BYTES
            && offered_writer_bytes.len() as u64 == expected.refresh.writer.size
            && offered_writer_bytes.len() as u64 <= manifest::MOST_FILE_BYTES
            && preimages.encoded().len() as u64 == expected.preimages.size
            && preimages.encoded().len() <= MOST_BUNDLE_BYTES
            && head_inputs.as_bytes().len() as u64 == expected.head_inputs.size
            && head_inputs.as_bytes().len() as u64 <= MOST_HEAD_INPUT_BYTES,
    )?;
    require(
        ring::digest::digest(&ring::digest::SHA256, trusted_key).as_ref()
            == expected.trusted_key_sha256.as_slice()
            && expected.envelope.matches(&Artifact::of(envelope))
            && preimages.transaction() == expected.transaction
            && expected.preimages.matches(preimages.artifact())
            && expected
                .preimages
                .matches(&Artifact::of(preimages.encoded()))
            && expected
                .head_inputs
                .matches(&Artifact::of(head_inputs.as_bytes())),
    )?;
    let (manifest, signed_bytes) =
        manifest::open_as(envelope, trusted_key, manifest::UPDATER).map_err(|_| Refused)?;
    require(
        manifest.channel == expected.channel
            && manifest.sequence == expected.sequence
            && manifest.commit == expected.commit,
    )?;
    supported(&manifest, &expected.refresh.writer)?;
    require(
        expected
            .refresh
            .writer
            .matches(&Artifact::of(offered_writer_bytes)),
    )?;
    require(
        manifest.settings.len() == preimages.entries().len()
            && manifest.settings.len() == expected.product.records.len(),
    )?;
    for ((setting, before), selected) in manifest
        .settings
        .iter()
        .zip(preimages.entries())
        .zip(expected.product.records)
    {
        require(
            setting.path == before.path()
                && setting.path == selected.path
                && before.recorded() == selected.recorded
                && before.writing() == selected.writing,
        )?;
    }
    coherent_bundle(&preimages)?;
    head_inputs
        .check_preimages(expected.root, &preimages)
        .map_err(|_| Refused)?;
    let mut recorded = BTreeMap::new();
    let mut writing = BTreeMap::new();
    for selected in expected.product.records {
        if let Some(value) = selected.recorded {
            recorded.insert(selected.path.to_owned(), value.to_owned());
        }
        if let Some(value) = selected.writing {
            writing.insert(selected.path.to_owned(), value.to_owned());
        }
    }
    let shipped = BTreeMap::new();
    let mut rows = Vec::with_capacity(manifest.settings.len());
    let mut steps = Vec::with_capacity(manifest.settings.len());
    for (index, (setting, before)) in manifest
        .settings
        .iter()
        .zip(preimages.entries())
        .enumerate()
    {
        let now = match &before.before().entry {
            Entry::Present { bytes, .. } => Some(manifest::sha256_hex(bytes)),
            Entry::MissingLeaf | Entry::MissingAncestor { .. } => None,
        };
        let act = apply::observed_act(setting, now.as_deref(), &recorded, &writing, &shipped);
        if let Some(act) = &act {
            steps.push(Step {
                setting: setting.clone(),
                act: act.clone(),
            });
        }
        rows.push(Row {
            signed_index: index,
            initial: act,
            effective: None,
        });
    }
    let carried = BTreeMap::new();
    let projected = head_touch::plan_output::project(
        &mut head_inputs,
        expected.root,
        expected.installed_units,
        &carried,
        &manifest.settings,
        steps,
    )
    .map_err(|_| Refused)?;
    head_touch::verify_with(
        &mut head_inputs,
        expected.root,
        expected.installed_units,
        &carried,
        &projected,
    )
    .map_err(|_| Refused)?;
    let projected_order = reconcile(&manifest.settings, &mut rows, projected)?;
    let has_content_changes = rows.iter().any(|row| {
        row.effective
            .as_ref()
            .is_some_and(|step| matches!(step.act, Act::Write | Act::Remove))
    });
    Ok(PreparedOrdinary {
        _expected: expected,
        envelope: envelope.to_vec().into_boxed_slice(),
        signed_bytes,
        manifest,
        preimages,
        head_inputs,
        rows,
        projected_order,
        has_content_changes,
    })
}

#[cfg(test)]
mod tests;
