//! Bounded, read-only head-policy inputs. No authority, locking or restoration.
//!
//! The caller owns a trusted stable root, including its ancestry/ownership, and
//! excludes writers throughout collection and use. Checks detect observations,
//! not an atomic filesystem snapshot or a kernel/storage latency deadline.
use super::{ConfEntries, HeadInputs};
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, Stat};
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::{OsStr, OsString},
    fmt,
    fs::File,
    io::{self, Read},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;
const REFUSED: &str = "head input capture refused";

fn checked<T, E>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|_| REFUSED.into())
}

#[derive(Clone, Copy)]
struct Limits {
    file: usize,
    bytes: usize,
    names: usize,
    name_bytes: usize,
    keys: usize,
    path: usize,
    components: usize,
    links: usize,
    work: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file: 1024 * 1024,
            bytes: 4 * 1024 * 1024,
            names: 128,
            name_bytes: 64 * 1024,
            keys: 256,
            path: 4096,
            components: 64,
            links: 16,
            work: 16 * 1024,
        }
    }
}

/// Ignore atime: our own read can advance it. Keep rdev for null witnesses.
#[derive(Clone, Copy)]
struct Identity(Stat);

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (&self.0, &other.0);
        a.st_dev == b.st_dev
            && a.st_ino == b.st_ino
            && a.st_mode == b.st_mode
            && a.st_nlink == b.st_nlink
            && a.st_uid == b.st_uid
            && a.st_gid == b.st_gid
            && a.st_rdev == b.st_rdev
            && a.st_size == b.st_size
            && a.st_mtime == b.st_mtime
            && a.st_mtime_nsec == b.st_mtime_nsec
            && a.st_ctime == b.st_ctime
            && a.st_ctime_nsec == b.st_ctime_nsec
    }
}
impl Eq for Identity {}

fn identity(file: &File) -> Result<Identity> {
    checked(rustix::fs::fstat(file)).map(Identity)
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
}

// Path's equality/order normalize '.' and trailing separators, while the
// kernel can require a directory for those spellings. Never collapse distinct
// filesystem requests into one cached key. Link targets have separate parsing.
fn key_path(path: &Path, limits: Limits) -> Result<()> {
    let bytes = path.as_os_str().as_bytes();
    if !bytes.starts_with(b"/") || bytes.len() > limits.path || bytes.contains(&0) {
        return Err(REFUSED.into());
    }
    if bytes == b"/" {
        return Ok(());
    }
    let mut count = 0;
    for component in bytes[1..].split(|byte| *byte == b'/') {
        count += 1;
        if component.is_empty() || matches!(component, b"." | b"..") || count > limits.components {
            return Err(REFUSED.into());
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
enum Witness {
    Directory(Identity),
    Link { identity: Identity, target: Vec<u8> },
}

#[derive(Clone, PartialEq, Eq)]
enum Text {
    Regular { text: String, identity: Identity },
    NullMask { leaf: Identity, null: Identity },
    MissingLeaf,
    MissingAncestor { suffix: Vec<OsString> },
}

impl Text {
    fn value(&self) -> Option<String> {
        match self {
            Self::Regular { text, .. } => Some(text.clone()),
            Self::NullMask { .. } => Some(String::new()),
            Self::MissingLeaf | Self::MissingAncestor { .. } => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct TextFact {
    witnesses: Vec<Witness>,
    text: Text,
}

#[derive(Clone)]
struct DirectoryFact {
    witnesses: Vec<Witness>,
    // None is an explicit missing path, not a directory with no members.
    names: Option<Vec<OsString>>,
    missing: Option<Text>,
}

impl DirectoryFact {
    fn equivalent(&self, other: &Self) -> bool {
        let sorted = |names: &Option<Vec<OsString>>| {
            names.as_ref().map(|names| {
                let mut names = names.clone();
                names.sort();
                names
            })
        };
        self.witnesses == other.witnesses
            && self.missing == other.missing
            && sorted(&self.names) == sorted(&other.names)
    }

    fn entries(&self, path: &Path) -> Option<ConfEntries> {
        self.names.as_ref().map(|names| {
            let paths: Vec<_> = names.iter().map(|name| Ok(path.join(name))).collect();
            Box::new(paths.into_iter()) as ConfEntries
        })
    }
}

/// Contains no descriptors and never performs filesystem I/O. Debug is counts
/// only; paths, unit/calibration contents and link targets stay private.
pub(crate) struct FrozenInputs {
    root_path: PathBuf,
    root_identity: Identity,
    limits: Limits,
    texts: BTreeMap<PathBuf, TextFact>,
    directories: BTreeMap<PathBuf, DirectoryFact>,
}

impl FrozenInputs {
    /// Fresh read-only comparison for the future mutation boundary. The caller
    /// still owns stable ancestry, exclusion and binding to the actual plan.
    pub(crate) fn revalidate(&self, root: &Path) -> Result<()> {
        if root != self.root_path {
            return Err(REFUSED.into());
        }
        let mut fresh = Collector::open(root, self.limits)?;
        if fresh.root_identity != self.root_identity {
            return Err(REFUSED.into());
        }
        for (path, expected) in &self.texts {
            fresh.key(path)?;
            if fresh.observe_text(path)? != *expected {
                return Err(REFUSED.into());
            }
        }
        for (path, expected) in &self.directories {
            fresh.key(path)?;
            if !fresh.observe_directory(path)?.equivalent(expected) {
                return Err(REFUSED.into());
            }
        }
        if identity(&fresh.root)? != self.root_identity {
            return Err(REFUSED.into());
        }
        Ok(())
    }
}

impl fmt::Debug for FrozenInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrozenInputs")
            .field("text_count", &self.texts.len())
            .field("directory_count", &self.directories.len())
            .finish_non_exhaustive()
    }
}

impl HeadInputs for FrozenInputs {
    fn text(&mut self, path: &Path) -> Result<Option<String>> {
        key_path(path, self.limits)?;
        self.texts
            .get(path)
            .map(|fact| fact.text.value())
            .ok_or_else(|| "uncaptured head text input".into())
    }

    fn entries(&mut self, path: &Path) -> Result<Option<ConfEntries>> {
        key_path(path, self.limits)?;
        self.directories
            .get(path)
            .map(|fact| fact.entries(path))
            .ok_or_else(|| "uncaptured head directory input".into())
    }
}

/// Returns the evaluator output only after every captured dependency is read
/// again and agrees. This does not make later filesystem mutation impossible.
/// Callers must bind the candidate/output and recheck at the mutation boundary.
/// The callback owns completeness and bounds on its carried in-memory inputs;
/// this API does not claim to have frozen every update/install read.
#[allow(
    dead_code,
    reason = "Staged read-only collector; the independent guard is not wired yet"
)]
pub(crate) fn collect<T>(
    root: &Path,
    evaluate: impl FnOnce(&mut dyn HeadInputs) -> Result<T>,
) -> Result<(T, FrozenInputs)> {
    let mut collector = Collector::open(root, Limits::default())?;
    let output = evaluate(&mut collector).map_err(|_| REFUSED.to_owned())?;
    Ok((output, collector.finish()?))
}

struct Collector {
    root_path: PathBuf,
    root: File,
    root_identity: Identity,
    limits: Limits,
    bytes: usize,
    name_bytes: usize,
    work: usize,
    failed: bool,
    texts: BTreeMap<PathBuf, TextFact>,
    directories: BTreeMap<PathBuf, DirectoryFact>,
}

enum Resolved {
    Node {
        parent: File,
        leaf: OsString,
        expected: Identity,
    },
    Directory(File),
    Missing(Text),
    Null(Identity),
}

impl Collector {
    fn open(root: &Path, limits: Limits) -> Result<Self> {
        // Require a lexical absolute root: no canonicalize or hidden link read.
        key_path(root, limits)?;
        let file = File::from(checked(rustix::fs::open(
            root,
            directory_flags(),
            Mode::empty(),
        ))?);
        let root_identity = identity(&file)?;
        Ok(Self {
            root_path: root.into(),
            root: file,
            root_identity,
            limits,
            bytes: 0,
            name_bytes: 0,
            work: 0,
            failed: false,
            texts: BTreeMap::new(),
            directories: BTreeMap::new(),
        })
    }

    fn work(&mut self, amount: usize) -> Result<()> {
        self.work = self.work.checked_add(amount).ok_or(REFUSED)?;
        if self.failed || self.work > self.limits.work {
            return Err(REFUSED.into());
        }
        Ok(())
    }

    fn key(&mut self, path: &Path) -> Result<()> {
        self.work(1)?;
        key_path(path, self.limits)?;
        if self.texts.len() + self.directories.len() >= self.limits.keys {
            return Err(REFUSED.into());
        }
        Ok(())
    }

    fn parts(&mut self, path: &Path, link: bool) -> Result<VecDeque<OsString>> {
        if path.as_os_str().as_bytes().len() > self.limits.path {
            return Err(REFUSED.into());
        }
        let mut parts = VecDeque::new();
        for component in path.as_os_str().as_bytes().split(|byte| *byte == b'/') {
            if component.is_empty() {
                continue;
            }
            if !link && matches!(component, b"." | b"..") {
                return Err(REFUSED.into());
            }
            let value = OsString::from_vec(component.to_vec());
            if value.as_bytes().contains(&0) {
                return Err(REFUSED.into());
            }
            parts.push_back(value);
            if parts.len() > self.limits.components {
                return Err(REFUSED.into());
            }
        }
        if link && path.as_os_str().as_bytes().ends_with(b"/") {
            parts.push_back(OsString::from("."));
            if parts.len() > self.limits.components {
                return Err(REFUSED.into());
            }
        }
        Ok(parts)
    }

    fn null() -> Result<Identity> {
        let null = Identity(checked(rustix::fs::statat(
            rustix::fs::CWD,
            "/dev/null",
            AtFlags::SYMLINK_NOFOLLOW,
        ))?);
        if FileType::from_raw_mode(null.0.st_mode) != FileType::CharacterDevice {
            return Err(REFUSED.into());
        }
        Ok(null)
    }

    fn resolve(&mut self, path: &Path) -> Result<(Resolved, Vec<Witness>)> {
        let relative = checked(path.strip_prefix(&self.root_path))?;
        let mut pending = self.parts(relative, false)?;
        let mut directories = vec![checked(self.root.try_clone())?];
        let mut witnesses = vec![Witness::Directory(identity(&self.root)?)];
        let mut links = 0;
        let mut expanded = pending.len();
        while let Some(part) = pending.pop_front() {
            self.work(1)?;
            if part.as_os_str() == OsStr::new(".") {
                continue;
            }
            if part.as_os_str() == OsStr::new("..") {
                if directories.len() == 1 {
                    return Err(REFUSED.into());
                }
                directories.pop();
                continue;
            }
            let parent = directories.last().ok_or(REFUSED)?;
            let expected = match rustix::fs::statat(parent, &part, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => Identity(stat),
                Err(rustix::io::Errno::NOENT) => {
                    let missing = if pending.is_empty() {
                        Text::MissingLeaf
                    } else {
                        Text::MissingAncestor {
                            suffix: std::iter::once(part).chain(pending).collect(),
                        }
                    };
                    return Ok((Resolved::Missing(missing), witnesses));
                }
                Err(_) => return Err(REFUSED.into()),
            };
            match FileType::from_raw_mode(expected.0.st_mode) {
                FileType::Symlink => {
                    links += 1;
                    if links > self.limits.links {
                        return Err(REFUSED.into());
                    }
                    let mut buffer = vec![0; self.limits.path + 1];
                    let length = checked(rustix::fs::readlinkat_raw(
                        parent,
                        &part,
                        buffer.as_mut_slice(),
                    ))?;
                    if length == 0 || length > self.limits.path {
                        return Err(REFUSED.into());
                    }
                    buffer.truncate(length);
                    let after = Identity(checked(rustix::fs::statat(
                        parent,
                        &part,
                        AtFlags::SYMLINK_NOFOLLOW,
                    ))?);
                    if after != expected {
                        return Err(REFUSED.into());
                    }
                    let target = PathBuf::from(OsString::from_vec(buffer.clone()));
                    witnesses.push(Witness::Link {
                        identity: expected,
                        target: buffer,
                    });
                    let relative_target = if target.is_absolute() {
                        // The sole outside-root exception is a terminal trusted
                        // null mask. Classification is metadata only.
                        if target.as_os_str().as_bytes() == b"/dev/null"
                            && pending.is_empty()
                            && !target.starts_with(&self.root_path)
                        {
                            return Ok((Resolved::Null(Self::null()?), witnesses));
                        }
                        // Keep raw trailing '/' and '.' semantics: Path's
                        // component iterator would silently normalize them.
                        let bytes = target.as_os_str().as_bytes();
                        let root = self.root_path.as_os_str().as_bytes();
                        let relative = if root == b"/" {
                            &bytes[1..]
                        } else if bytes == root {
                            &bytes[bytes.len()..]
                        } else {
                            let suffix = bytes.strip_prefix(root).ok_or(REFUSED)?;
                            suffix.strip_prefix(b"/").ok_or(REFUSED)?
                        };
                        directories.truncate(1);
                        PathBuf::from(OsString::from_vec(relative.to_vec()))
                    } else {
                        target
                    };
                    let mut next = self.parts(&relative_target, true)?;
                    expanded = expanded.checked_add(next.len()).ok_or(REFUSED)?;
                    if expanded > self.limits.components {
                        return Err(REFUSED.into());
                    }
                    self.work(next.len())?;
                    next.append(&mut pending);
                    pending = next;
                }
                FileType::Directory => {
                    let file = File::from(checked(rustix::fs::openat(
                        parent,
                        &part,
                        directory_flags(),
                        Mode::empty(),
                    ))?);
                    if identity(&file)? != expected {
                        return Err(REFUSED.into());
                    }
                    witnesses.push(Witness::Directory(expected));
                    directories.push(file);
                }
                _ if pending.is_empty() => {
                    return Ok((
                        Resolved::Node {
                            parent: checked(parent.try_clone())?,
                            leaf: part,
                            expected,
                        },
                        witnesses,
                    ));
                }
                _ => return Err(REFUSED.into()),
            }
        }
        Ok((
            Resolved::Directory(directories.pop().ok_or(REFUSED)?),
            witnesses,
        ))
    }

    fn observe_text(&mut self, path: &Path) -> Result<TextFact> {
        let (node, witnesses) = self.resolve(path)?;
        let text = match node {
            Resolved::Missing(text) => text,
            Resolved::Null(null) => Text::NullMask { leaf: null, null },
            Resolved::Node {
                parent,
                leaf,
                expected,
            } => match FileType::from_raw_mode(expected.0.st_mode) {
                FileType::RegularFile => {
                    let remaining = self.limits.bytes.checked_sub(self.bytes).ok_or(REFUSED)?;
                    let text = read_regular(
                        &parent,
                        &leaf,
                        expected,
                        remaining.min(self.limits.file),
                        || {},
                        || {},
                    )?;
                    self.bytes = self.bytes.checked_add(text.len()).ok_or(REFUSED)?;
                    Text::Regular {
                        text,
                        identity: expected,
                    }
                }
                FileType::CharacterDevice => {
                    let null = Self::null()?;
                    if expected.0.st_rdev != null.0.st_rdev {
                        return Err(REFUSED.into());
                    }
                    Text::NullMask {
                        leaf: expected,
                        null,
                    }
                }
                _ => return Err(REFUSED.into()),
            },
            Resolved::Directory(_) => return Err(REFUSED.into()),
        };
        Ok(TextFact { witnesses, text })
    }

    fn observe_directory(&mut self, path: &Path) -> Result<DirectoryFact> {
        let (node, witnesses) = self.resolve(path)?;
        let directory = match node {
            Resolved::Missing(text) => {
                return Ok(DirectoryFact {
                    witnesses,
                    names: None,
                    missing: Some(text),
                });
            }
            Resolved::Directory(directory) => directory,
            _ => return Err(REFUSED.into()),
        };
        let before = identity(&directory)?;
        let mut names = Vec::new();
        for entry in checked(Dir::read_from(&directory))? {
            self.work(1)?;
            let entry = checked(entry)?;
            let name = entry.file_name().to_bytes();
            if matches!(name, b"." | b"..") {
                continue;
            }
            self.name_bytes = self.name_bytes.checked_add(name.len()).ok_or(REFUSED)?;
            if names.len() >= self.limits.names || self.name_bytes > self.limits.name_bytes {
                return Err(REFUSED.into());
            }
            if path.as_os_str().as_bytes().len() + 1 + name.len() > self.limits.path {
                return Err(REFUSED.into());
            }
            if names
                .iter()
                .any(|previous: &OsString| previous.as_bytes() == name)
            {
                return Err(REFUSED.into());
            }
            // Raw names are retained; no lossy decoding or filtering here.
            names.push(OsString::from_vec(name.to_vec()));
        }
        if identity(&directory)? != before {
            return Err(REFUSED.into());
        }
        Ok(DirectoryFact {
            witnesses,
            names: Some(names),
            missing: None,
        })
    }

    fn finish(self) -> Result<FrozenInputs> {
        if self.failed {
            return Err(REFUSED.into());
        }
        if identity(&self.root)? != self.root_identity {
            return Err(REFUSED.into());
        }
        let frozen = FrozenInputs {
            root_path: self.root_path,
            root_identity: self.root_identity,
            limits: self.limits,
            texts: self.texts,
            directories: self.directories,
        };
        frozen.revalidate(&frozen.root_path)?;
        Ok(frozen)
    }
}

impl HeadInputs for Collector {
    fn text(&mut self, path: &Path) -> Result<Option<String>> {
        let result = (|| {
            self.work(1)?;
            key_path(path, self.limits)?;
            if let Some(fact) = self.texts.get(path) {
                return Ok(fact.text.value());
            }
            self.key(path)?;
            let fact = self.observe_text(path)?;
            let value = fact.text.value();
            self.texts.insert(path.into(), fact);
            Ok(value)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn entries(&mut self, path: &Path) -> Result<Option<ConfEntries>> {
        let result = (|| {
            self.work(1)?;
            key_path(path, self.limits)?;
            if let Some(fact) = self.directories.get(path) {
                return Ok(fact.entries(path));
            }
            self.key(path)?;
            let fact = self.observe_directory(path)?;
            let value = fact.entries(path);
            self.directories.insert(path.into(), fact);
            Ok(value)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

// Reuse the existing strict observer's descriptor identity/read pattern, but
// allow regular hard links: their observed link count is part of the witness.
fn read_regular(
    parent: &File,
    leaf: &OsStr,
    expected: Identity,
    cap: usize,
    before_open: impl FnOnce(),
    after_read: impl FnOnce(),
) -> Result<String> {
    let length = usize::try_from(expected.0.st_size).map_err(|_| REFUSED)?;
    if length > cap {
        return Err(REFUSED.into());
    }
    before_open();
    let flags =
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC;
    let mut file = File::from(checked(rustix::fs::openat(
        parent,
        leaf,
        flags,
        Mode::empty(),
    ))?);
    if identity(&file)? != expected {
        return Err(REFUSED.into());
    }
    let mut bytes = vec![0; length + 1];
    let mut used = 0;
    while used < bytes.len() {
        match file.read(&mut bytes[used..]) {
            Ok(0) => break,
            Ok(count) => used += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(REFUSED.into()),
        }
    }
    after_read();
    let named = Identity(checked(rustix::fs::statat(
        parent,
        leaf,
        AtFlags::SYMLINK_NOFOLLOW,
    ))?);
    if used != length || identity(&file)? != expected || named != expected {
        return Err(REFUSED.into());
    }
    bytes.truncate(used);
    checked(String::from_utf8(bytes))
}

#[cfg(test)]
mod tests;
