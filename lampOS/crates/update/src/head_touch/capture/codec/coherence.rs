//! Represented lexical agreement only, with no live reads or admission.
use super::*;
use crate::{
    writer_files::{Entry as RawEntry, Identity as RawIdentity, Observation},
    writer_preimages::Bundle,
};

const DISAGREE: &str = "saved head and preimages disagree";

fn agree(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(DISAGREE.into())
    }
}

// Compare only the fields both formats retain. Raw has no rdev or atime.
// In particular, Apple's negative dev_t is not bitcast into a u64 identity.
fn same(stamp: &Stamp, raw: &RawIdentity) -> bool {
    u64::try_from(stamp.dev).ok() == Some(raw.device)
        && stamp.ino == raw.inode
        && stamp.mode == raw.mode
        && stamp.nlink == raw.links
        && stamp.uid == raw.uid
        && stamp.gid == raw.gid
        && u64::try_from(stamp.size).ok() == Some(raw.length)
        && stamp.mtime == raw.modified_seconds
        && i64::from(stamp.mtime_ns) == raw.modified_nanos
        && stamp.ctime == raw.changed_seconds
        && i64::from(stamp.ctime_ns) == raw.changed_nanos
}

enum Terminal<'a> {
    Text(&'a SavedText),
    Directory(&'a [Raw]),
}

enum Prefix<'a> {
    Directory(&'a Stamp),
    Regular(&'a Stamp, &'a [u8]),
    Absent,
}

// Both input formats already validate canonical components and finite sizes.
// Keep exact raw spelling rather than Path equality's normalization behavior.
fn relative<'a>(root: &[u8], path: &'a [u8]) -> Result<Vec<&'a [u8]>> {
    let tail = if path == root {
        &path[path.len()..]
    } else if root == b"/" {
        path.strip_prefix(b"/".as_slice()).ok_or(DISAGREE)?
    } else {
        path.strip_prefix(root)
            .and_then(|tail| tail.strip_prefix(b"/".as_slice()))
            .ok_or(DISAGREE)?
    };
    if tail.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<_> = tail.split(|byte| *byte == b'/').collect();
    agree(
        parts.len() <= 64
            && parts
                .iter()
                .all(|p| !p.is_empty() && !matches!(*p, b"." | b"..")),
    )?;
    Ok(parts)
}

fn raw_missing(parts: usize, observed: &Observation) -> Option<usize> {
    match &observed.entry {
        RawEntry::Present { .. } => None,
        RawEntry::MissingLeaf => Some(parts),
        RawEntry::MissingAncestor { component } => Some(component + 1),
    }
}

fn compare_prefix(head: Prefix<'_>, raw: &Observation, index: usize) -> Result<()> {
    if let Some(identity) = raw.ancestors.get(index) {
        return agree(matches!(head, Prefix::Directory(stamp) if same(stamp, identity)));
    }
    match (&raw.entry, head) {
        (RawEntry::Present { bytes, identity }, Prefix::Regular(stamp, text)) => {
            agree(same(stamp, identity) && bytes == text)
        }
        (RawEntry::MissingLeaf | RawEntry::MissingAncestor { .. }, Prefix::Absent) => Ok(()),
        _ => Err(DISAGREE.into()),
    }
}

fn compare_row(
    root: &[u8],
    path: &[u8],
    witnesses: &[SavedWitness],
    terminal: Terminal<'_>,
    raw_parts: &[&[u8]],
    raw: &Observation,
) -> Result<()> {
    let parts = relative(root, path)?;
    let common = parts
        .iter()
        .zip(raw_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if let Some(link) = witnesses
        .iter()
        .position(|w| matches!(w, SavedWitness::Link { .. }))
    {
        // The wire reader admits bounded facts, not this ordinary-plan
        // mapping. A first link must name an actual lexical component; an
        // extra witness past the leaf must not hide membership/type checks.
        agree(link > 0 && link <= parts.len())?;
        // Before the first link, directory witnesses retain lexical meaning.
        // Resolving this link or any alias after it is outside this protocol.
        agree(link > common)?;
        for (index, witness) in witnesses.iter().take(common + 1).enumerate() {
            let SavedWitness::Directory(stamp) = witness else {
                return Err(DISAGREE.into());
            };
            compare_prefix(Prefix::Directory(stamp), raw, index)?;
        }
        return Ok(());
    }

    let missing = match &terminal {
        Terminal::Text(SavedText::MissingLeaf) => Some(parts.len()),
        Terminal::Text(SavedText::MissingAncestor { suffix }) => {
            let start = parts.len().checked_sub(suffix.len()).ok_or(DISAGREE)?;
            agree(suffix.len() >= 2 && suffix.iter().zip(&parts[start..]).all(|(a, b)| a.0 == *b))?;
            Some(start + 1)
        }
        _ => None,
    };
    let wanted = match &terminal {
        Terminal::Directory(_) => parts.len() + 1,
        Terminal::Text(_) => missing.unwrap_or(parts.len()),
    };
    agree(witnesses.len() == wanted)?;
    for index in 0..=common {
        let prefix = if let Some(SavedWitness::Directory(stamp)) = witnesses.get(index) {
            Prefix::Directory(stamp)
        } else if missing.is_some_and(|first| index >= first) {
            Prefix::Absent
        } else {
            match &terminal {
                Terminal::Text(SavedText::Regular { text, stamp }) if index == parts.len() => {
                    Prefix::Regular(stamp, text.as_bytes())
                }
                // Null/device facts have no ordinary Raw-file counterpart.
                _ => return Err(DISAGREE.into()),
            }
        };
        compare_prefix(prefix, raw, index)?;
    }
    if parts == raw_parts {
        match (&terminal, &raw.entry) {
            (Terminal::Text(SavedText::MissingLeaf), RawEntry::MissingLeaf)
            | (
                Terminal::Text(SavedText::MissingAncestor { .. }),
                RawEntry::MissingAncestor { .. },
            )
            | (Terminal::Text(SavedText::Regular { .. }), RawEntry::Present { .. }) => {}
            _ => return Err(DISAGREE.into()),
        }
    }
    if common == parts.len()
        && raw_parts.len() > parts.len()
        && let Terminal::Directory(names) = terminal
    {
        let next = raw_parts[parts.len()];
        let exists = raw_missing(raw_parts.len(), raw).is_none_or(|first| parts.len() + 1 < first);
        agree(names.iter().any(|name| name.0 == next) == exists)?;
    }
    Ok(())
}

impl SavedInputs {
    /// Compare represented root, prefix and overlapping ordinary-file facts.
    /// This never reads the filesystem, changes either encoding, authenticates
    /// inputs or establishes freshness, writer exclusion or an atomic snapshot.
    /// Bundle has no historical root spelling: protected caller intent must
    /// bind both artifacts to this exact logical root independently.
    #[allow(
        dead_code,
        reason = "Private finite-plan prerequisite; no runtime caller is wired"
    )]
    pub(crate) fn check_preimages(&self, root: &Path, bundle: &Bundle) -> Result<()> {
        agree(root.as_os_str().as_bytes() == self.facts.root.0 && !bundle.entries().is_empty())?;
        for entry in bundle.entries() {
            let raw = entry.before();
            agree(
                raw.ancestors
                    .first()
                    .is_some_and(|stamp| same(&self.facts.root_stamp, stamp)),
            )?;
            let parts: Vec<_> = entry.path().as_bytes()[1..]
                .split(|byte| *byte == b'/')
                .collect();
            for row in &self.facts.texts {
                compare_row(
                    &self.facts.root.0,
                    &row.path.0,
                    &row.witnesses,
                    Terminal::Text(&row.fact),
                    &parts,
                    raw,
                )?;
            }
            for row in &self.facts.directories {
                let terminal = match (&row.names, &row.missing) {
                    (Some(names), None) => Terminal::Directory(names),
                    (
                        None,
                        Some(
                            missing @ (SavedText::MissingLeaf | SavedText::MissingAncestor { .. }),
                        ),
                    ) => Terminal::Text(missing),
                    _ => return Err(DISAGREE.into()),
                };
                compare_row(
                    &self.facts.root.0,
                    &row.path.0,
                    &row.witnesses,
                    terminal,
                    &parts,
                    raw,
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
