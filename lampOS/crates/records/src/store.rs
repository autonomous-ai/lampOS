//! Descriptor-relative storage for the current continuous voice record layout.
//!
//! This is deliberately not a recursive file remover. Accepted sessions contain
//! only `metadata/{events-0..3.jsonl,health.json}` (a partial set is allowed after
//! a failed start). The configured root and all ancestors must be trusted. A
//! producer must exclusively create its session and never reuse a stopped one;
//! the caller must exclude every active producer after taking the inventory.
//! Same-UID/root namespace attackers and mount administrators are outside that
//! contract: an open directory descriptor survives a move outside its old path.

use crate::{Disk, plan::Session};
use rustix::fs::{self, AtFlags, FileType, FlockOperation, Mode, OFlags, RenameFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

const MAX_SESSIONS: usize = 16_384;
const TRASH: &str = ".lamp-records-trash";
const RECORDS: [&str; 5] = [
    "events-0.jsonl",
    "events-1.jsonl",
    "events-2.jsonl",
    "events-3.jsonl",
    "health.json",
];

#[derive(Debug)]
pub struct Inventory {
    pub sessions: Vec<Session>,
    pub refused: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Removal {
    /// Allocated bytes of successfully unlinked entries, sampled before unlink.
    /// This includes directories and does not claim a measured increase in free
    /// space: open files, snapshots, and concurrent writers can retain blocks.
    pub removed_bytes: u64,
    /// Number of removed regular files and directories, excluding quarantine.
    pub removed_files: u64,
    pub complete: bool,
    pub error: Option<String>,
}

pub struct Store {
    root: File,
    parent: File,
    name: OsString,
    // Tests supply a pinned private fixture below a shared temporary directory.
    // Public construction always checks the complete absolute ancestor chain.
    path: Option<PathBuf>,
    identity: Node,
    owner: u32,
    snapshots: BTreeMap<String, Candidate>,
    #[cfg(test)]
    failure: Option<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    // i128 normalizes the signed/unsigned stat field types on Linux and macOS.
    dev: i128,
    ino: i128,
    mode: i128,
    uid: i128,
    gid: i128,
    nlink: i128,
    size: i128,
    blocks: i128,
    mtime: (i128, i128),
    ctime: (i128, i128),
    kind: FileType,
}

impl Node {
    fn from_stat(stat: fs::Stat) -> Self {
        Self {
            dev: i128::from(stat.st_dev),
            ino: i128::from(stat.st_ino),
            mode: i128::from(stat.st_mode),
            uid: i128::from(stat.st_uid),
            gid: i128::from(stat.st_gid),
            nlink: i128::from(stat.st_nlink),
            size: i128::from(stat.st_size),
            blocks: i128::from(stat.st_blocks),
            mtime: (i128::from(stat.st_mtime), i128::from(stat.st_mtime_nsec)),
            ctime: (i128::from(stat.st_ctime), i128::from(stat.st_ctime_nsec)),
            kind: FileType::from_raw_mode(stat.st_mode),
        }
    }

    fn of(fd: &File) -> io::Result<Self> {
        Ok(Self::from_stat(fs::fstat(fd)?))
    }

    fn at(parent: &File, name: &OsStr) -> io::Result<Self> {
        Ok(Self::from_stat(fs::statat(
            parent,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?))
    }

    fn same_object(&self, other: &Self) -> bool {
        (self.dev, self.ino, self.kind) == (other.dev, other.ino, other.kind)
    }

    fn bytes(&self) -> io::Result<u64> {
        u64::try_from(self.blocks)
            .ok()
            .and_then(|blocks| blocks.checked_mul(512))
            .ok_or_else(|| invalid("allocated byte count is invalid or overflows"))
    }

    fn unix_s(&self) -> io::Result<u64> {
        u64::try_from(self.mtime.0).map_err(|_| invalid("mtime is outside Unix seconds"))
    }

    fn directory(&self, owner: u32, dev: i128) -> io::Result<()> {
        if self.kind != FileType::Directory
            || self.uid != i128::from(owner)
            || self.mode & 0o7777 != 0o700
            || self.dev != dev
        {
            return Err(invalid(
                "directory must be owned, private, and on the root filesystem",
            ));
        }
        Ok(())
    }

    fn record(&self, owner: u32, dev: i128) -> io::Result<()> {
        if self.kind != FileType::RegularFile
            || self.uid != i128::from(owner)
            || self.mode & 0o7777 != 0o600
            || self.nlink != 1
            || self.dev != dev
        {
            return Err(invalid(
                "record must be an owned private regular file with one link",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tree {
    session: Node,
    metadata: Option<Node>,
    files: BTreeMap<String, Node>,
}

impl Tree {
    fn session(&self, id: &str, pending: bool) -> io::Result<Session> {
        let mut bytes = self.session.bytes()?;
        let mut newest_unix_s = self.session.unix_s()?;
        for node in self.metadata.iter().chain(self.files.values()) {
            bytes = bytes
                .checked_add(node.bytes()?)
                .ok_or_else(|| invalid("session allocation count overflows"))?;
            newest_unix_s = newest_unix_s.max(node.unix_s()?);
        }
        Ok(Session {
            id: id.to_owned(),
            bytes,
            newest_unix_s,
            pending,
        })
    }

    fn matches_after_rename(&self, actual: &Self) -> bool {
        // Rename changes this directory's ctime on Linux. No other mutation is
        // expected; especially preserve the pre-rename mtime used for age.
        let mut expected = self.clone();
        expected.session.ctime = actual.session.ctime;
        expected == *actual
    }
}

struct OpenTree {
    session: File,
    metadata: Option<File>,
    tree: Tree,
}

#[derive(Clone)]
struct Candidate {
    tree: Tree,
    pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mutation {
    Mkdir,
    Rename,
    Unlink,
    Sync,
}

impl Store {
    /// Open an existing absolute root whose ancestors are owned by root or the
    /// effective user and are not group/world writable, except root-owned
    /// sticky ancestors such as /tmp. Subsequent entries still require trusted
    /// ownership, so untrusted users cannot replace them under a sticky parent.
    /// The root itself must be owned by the effective user and mode 0700. No
    /// lock file is created.
    ///
    /// Linux requires openat2/NO_XDEV and O_NOATIME; unsupported kernels fail
    /// closed. The macOS fallback is for local development tests only and makes
    /// no Linux mount-confinement or unchanged-directory-atime claim.
    pub fn open(root: &Path) -> io::Result<Self> {
        let owner = rustix::process::geteuid().as_raw();
        let (parent, name, directory) = trusted_root(root, owner)?;
        Self::from_open(parent, name, directory, Some(root.to_owned()), owner)
    }

    fn from_open(
        parent: File,
        name: OsString,
        root: File,
        path: Option<PathBuf>,
        owner: u32,
    ) -> io::Result<Self> {
        let identity = Node::of(&root)?;
        identity.directory(owner, identity.dev)?;
        if Node::at(&parent, &name)? != identity {
            return Err(invalid("root changed while opening it"));
        }
        // Probe the required Linux resolver even if there are no sessions.
        drop(child_directory(&root, OsStr::new("."))?);
        fs::flock(&root, FlockOperation::NonBlockingLockExclusive)?;
        Ok(Self {
            root,
            parent,
            name,
            path,
            identity,
            owner,
            snapshots: BTreeMap::new(),
            #[cfg(test)]
            failure: None,
        })
    }

    fn check_root(&self) -> io::Result<()> {
        let current = Node::of(&self.root)?;
        current.directory(self.owner, self.identity.dev)?;
        if !self.identity.same_object(&current)
            || !current.same_object(&Node::at(&self.parent, &self.name)?)
        {
            return Err(invalid("configured root was replaced or moved"));
        }
        if let Some(path) = &self.path {
            let (_, _, reopened) = trusted_root(path, self.owner)?;
            if !current.same_object(&Node::of(&reopened)?) {
                return Err(invalid("configured root ancestry changed"));
            }
        }
        Ok(())
    }

    /// Fix the candidate identities before the caller obtains active-session
    /// authority. Holds at most a small constant number of FDs, independently
    /// of the number of sessions; reads no transcript or record contents.
    pub fn inventory(&mut self) -> io::Result<Inventory> {
        self.inventory_bounded(MAX_SESSIONS)
    }

    fn inventory_bounded(&mut self, limit: usize) -> io::Result<Inventory> {
        self.snapshots.clear();
        self.check_root()?;
        let root_names = names(&self.root, limit + 1)?;
        let mut budget = limit;
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        let mut snapshots = BTreeMap::new();
        let mut refused = Vec::new();
        for name in root_names.iter().filter(|name| *name != TRASH) {
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| invalid("inventory entry limit exceeded"))?;
            self.inspect(
                &self.root,
                name,
                false,
                &mut seen,
                &mut duplicates,
                &mut snapshots,
                &mut refused,
            );
        }
        if root_names.iter().any(|name| name == TRASH) {
            match self.quarantine() {
                Ok(trash) => {
                    for name in names(&trash, budget)? {
                        self.inspect(
                            &trash,
                            &name,
                            true,
                            &mut seen,
                            &mut duplicates,
                            &mut snapshots,
                            &mut refused,
                        );
                    }
                }
                Err(error) => {
                    // An invalid container is itself a reported refusal. It
                    // must share the entry budget with ordinary candidates,
                    // including candidates that a later plan/apply may refuse.
                    if budget == 0 {
                        return Err(invalid(
                            "invalid quarantine exceeds the inventory entry limit",
                        ));
                    }
                    refused.push(format!("{TRASH}: {error}"));
                }
            }
        }
        for id in duplicates {
            snapshots.remove(&id);
            refused.push(format!("{id}: both live and quarantine names exist"));
        }
        self.check_root()?;
        let sessions = snapshots
            .iter()
            .map(|(id, candidate)| candidate.tree.session(id, candidate.pending))
            .collect::<io::Result<Vec<_>>>()?;
        for message in &mut refused {
            bound_refusal(message);
        }
        refused.sort();
        self.snapshots = snapshots;
        Ok(Inventory { sessions, refused })
    }

    #[allow(clippy::too_many_arguments)] // One bounded scan shares its duplicate-name state.
    fn inspect(
        &self,
        parent: &File,
        name: &OsStr,
        pending: bool,
        seen: &mut BTreeSet<String>,
        duplicates: &mut BTreeSet<String>,
        snapshots: &mut BTreeMap<String, Candidate>,
        refused: &mut Vec<String>,
    ) {
        let Some(id) = session_id(name) else {
            refused.push(format!("{:?}: unknown entry name", name));
            return;
        };
        if !seen.insert(id.to_owned()) {
            duplicates.insert(id.to_owned());
            return;
        }
        match self.read_tree(parent, name, pending) {
            Ok(opened) => {
                snapshots.insert(
                    id.to_owned(),
                    Candidate {
                        tree: opened.tree,
                        pending,
                    },
                );
            }
            Err(error) => refused.push(format!("{id}: {error}")),
        }
    }

    fn read_tree(&self, parent: &File, name: &OsStr, pending: bool) -> io::Result<OpenTree> {
        let session = child_directory(parent, name)?;
        let session_node = Node::of(&session)?;
        session_node.directory(self.owner, self.identity.dev)?;
        if Node::at(parent, name)? != session_node {
            return Err(invalid("session changed while opening it"));
        }
        let children = names(&session, 1)?;
        let mut metadata = None;
        let mut metadata_node = None;
        let mut files = BTreeMap::new();
        if children.as_slice() == [OsString::from("metadata")] {
            let directory = child_directory(&session, OsStr::new("metadata"))?;
            let before = Node::of(&directory)?;
            before.directory(self.owner, self.identity.dev)?;
            if Node::at(&session, OsStr::new("metadata"))? != before {
                return Err(invalid("metadata directory changed while opening it"));
            }
            for file_name in names(&directory, RECORDS.len())? {
                let Some(file_name) = file_name.to_str().filter(|name| RECORDS.contains(name))
                else {
                    return Err(invalid("unknown entry in metadata directory"));
                };
                let node = record_node(&directory, OsStr::new(file_name))?;
                node.record(self.owner, self.identity.dev)?;
                files.insert(file_name.to_owned(), node);
            }
            // Recheck the collected names and nodes: reject a writer observed
            // during inventory, instead of publishing a mixed-age snapshot.
            if names(&directory, RECORDS.len())?
                != files.keys().map(OsString::from).collect::<Vec<_>>()
                || Node::of(&directory)? != before
                || Node::at(&session, OsStr::new("metadata"))? != before
            {
                return Err(invalid("metadata directory changed during inventory"));
            }
            for (file_name, node) in &files {
                if record_node(&directory, OsStr::new(file_name))? != *node {
                    return Err(invalid("record changed during inventory"));
                }
            }
            metadata_node = Some(before);
            metadata = Some(directory);
        } else if !(pending && children.is_empty()) {
            return Err(invalid("session must contain only its metadata directory"));
        }
        if Node::of(&session)? != session_node || Node::at(parent, name)? != session_node {
            return Err(invalid("session changed during inventory"));
        }
        Ok(OpenTree {
            session,
            metadata,
            tree: Tree {
                session: session_node,
                metadata: metadata_node,
                files,
            },
        })
    }

    fn quarantine(&self) -> io::Result<File> {
        let trash = child_directory(&self.root, OsStr::new(TRASH))?;
        let node = Node::of(&trash)?;
        node.directory(self.owner, self.identity.dev)?;
        if Node::at(&self.root, OsStr::new(TRASH))? != node {
            return Err(invalid("quarantine directory changed while opening it"));
        }
        Ok(trash)
    }

    fn create_quarantine(&mut self) -> io::Result<File> {
        self.mutation(Mutation::Mkdir)?;
        match fs::mkdirat(&self.root, TRASH, Mode::from_bits_truncate(0o700)) {
            Ok(()) => {
                self.mutation(Mutation::Sync)?;
                fs::fsync(&self.root)?;
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
        self.quarantine()
    }

    /// Remove only an ID accepted by the last inventory. The caller must have
    /// established that it is inactive since that inventory. Each ID is consumed
    /// on its first attempt, including an error; retry requires a new inventory
    /// and fresh active-session authority.
    ///
    /// A failure before any unlink is Err (a safe quarantined tree can remain).
    /// After any unlink, the actual count is returned with complete=false and
    /// an error, so a partially completed transaction is never reported as zero.
    pub fn remove(&mut self, id: &str) -> io::Result<Removal> {
        let candidate = self
            .snapshots
            .remove(id)
            .ok_or_else(|| invalid("session is not in the current inventory"))?;
        let mut removal = Removal::default();
        let result = self.remove_candidate(id, &candidate, &mut removal);
        match result {
            Ok(()) => {
                removal.complete = true;
                Ok(removal)
            }
            Err(error) if removal.removed_files != 0 => {
                removal.error = Some(error.to_string());
                Ok(removal)
            }
            Err(error) => Err(error),
        }
    }

    fn remove_candidate(
        &mut self,
        id: &str,
        candidate: &Candidate,
        removal: &mut Removal,
    ) -> io::Result<()> {
        self.check_root()?;
        let trash;
        let opened;
        if candidate.pending {
            trash = self.quarantine()?;
            match Node::at(&self.root, OsStr::new(id)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
                Ok(_) => return Err(invalid("pending session also has a live name")),
            }
            opened = self.read_tree(&trash, OsStr::new(id), true)?;
            if opened.tree != candidate.tree {
                return Err(invalid("pending session changed since inventory"));
            }
        } else {
            let original = self.read_tree(&self.root, OsStr::new(id), false)?;
            if original.tree != candidate.tree {
                return Err(invalid("session changed since inventory"));
            }
            trash = self.create_quarantine()?;
            self.check_root()?;
            if Node::at(&self.root, OsStr::new(id))? != candidate.tree.session {
                return Err(invalid("session changed before quarantine"));
            }
            self.mutation(Mutation::Rename)?;
            fs::renameat_with(&self.root, id, &trash, id, RenameFlags::NOREPLACE)?;
            // Do not roll back on a mismatch: that might overwrite a new name.
            // No unlink occurs until the whole renamed tree is validated.
            opened = self.read_tree(&trash, OsStr::new(id), true)?;
            if !candidate.tree.matches_after_rename(&opened.tree)
                || !Node::of(&original.session)?.same_object(&opened.tree.session)
            {
                return Err(invalid("quarantined session differs from the inventory"));
            }
            self.mutation(Mutation::Sync)?;
            fs::fsync(&self.root)?;
            self.mutation(Mutation::Sync)?;
            fs::fsync(&trash)?;
        }
        self.check_root()?;
        self.check_quarantine(&trash)?;
        if Node::at(&trash, OsStr::new(id))? != opened.tree.session {
            return Err(invalid("quarantined session was replaced"));
        }
        // Fsync can be slow. Validate every remaining entry once more after
        // those calls and before the first unlink, not just the first file.
        if self.read_tree(&trash, OsStr::new(id), true)?.tree != opened.tree {
            return Err(invalid("quarantined contents changed before deletion"));
        }
        if let Some(metadata) = &opened.metadata {
            for (name, expected) in &opened.tree.files {
                self.check_root()?;
                self.check_quarantine(&trash)?;
                require_link(&trash, OsStr::new(id), &opened.tree.session)?;
                require_link(
                    &opened.session,
                    OsStr::new("metadata"),
                    opened.tree.metadata.as_ref().expect("opened metadata"),
                )?;
                let current = record_node(metadata, OsStr::new(name))?;
                if current != *expected {
                    return Err(invalid("record changed before unlink"));
                }
                self.unlink(
                    metadata,
                    OsStr::new(name),
                    AtFlags::empty(),
                    &current,
                    removal,
                )?;
            }
            self.mutation(Mutation::Sync)?;
            fs::fsync(metadata)?;
            self.check_root()?;
            self.check_quarantine(&trash)?;
            require_link(&trash, OsStr::new(id), &opened.tree.session)?;
            require_link(
                &opened.session,
                OsStr::new("metadata"),
                opened.tree.metadata.as_ref().expect("opened metadata"),
            )?;
            if !names(metadata, 0)?.is_empty() {
                return Err(invalid("metadata directory is no longer empty"));
            }
            let current = Node::of(metadata)?;
            self.unlink(
                &opened.session,
                OsStr::new("metadata"),
                AtFlags::REMOVEDIR,
                &current,
                removal,
            )?;
            self.mutation(Mutation::Sync)?;
            fs::fsync(&opened.session)?;
        }
        self.check_root()?;
        self.check_quarantine(&trash)?;
        require_link(&trash, OsStr::new(id), &opened.tree.session)?;
        if !names(&opened.session, 0)?.is_empty() {
            return Err(invalid("session directory is no longer empty"));
        }
        let current = Node::of(&opened.session)?;
        self.unlink(
            &trash,
            OsStr::new(id),
            AtFlags::REMOVEDIR,
            &current,
            removal,
        )?;
        self.mutation(Mutation::Sync)?;
        fs::fsync(&trash)?;
        Ok(())
    }

    fn check_quarantine(&self, trash: &File) -> io::Result<()> {
        let current = Node::of(trash)?;
        current.directory(self.owner, self.identity.dev)?;
        if !current.same_object(&Node::at(&self.root, OsStr::new(TRASH))?) {
            return Err(invalid("quarantine was replaced or moved"));
        }
        Ok(())
    }

    fn unlink(
        &mut self,
        parent: &File,
        name: &OsStr,
        flags: AtFlags,
        node: &Node,
        removal: &mut Removal,
    ) -> io::Result<()> {
        let bytes = removal
            .removed_bytes
            .checked_add(node.bytes()?)
            .ok_or_else(|| invalid("removed allocation count overflows"))?;
        let files = removal
            .removed_files
            .checked_add(1)
            .ok_or_else(|| invalid("removed entry count overflows"))?;
        self.mutation(Mutation::Unlink)?;
        fs::unlinkat(parent, name, flags)?;
        removal.removed_bytes = bytes;
        removal.removed_files = files;
        Ok(())
    }

    pub fn disk(&self) -> io::Result<Disk> {
        self.check_root()?;
        let stat = fs::fstatvfs(&self.root)?;
        if stat.f_frsize == 0 || stat.f_blocks == 0 || stat.f_bavail > stat.f_blocks {
            return Err(invalid("filesystem capacity counters are invalid"));
        }
        Ok(Disk {
            total_bytes: stat
                .f_blocks
                .checked_mul(stat.f_frsize)
                .ok_or_else(|| invalid("filesystem total bytes overflow"))?,
            available_bytes: stat
                .f_bavail
                .checked_mul(stat.f_frsize)
                .ok_or_else(|| invalid("filesystem available bytes overflow"))?,
        })
    }

    fn mutation(&mut self, operation: Mutation) -> io::Result<()> {
        #[cfg(test)]
        if let Some(failure) = &mut self.failure
            && failure.operation == operation
        {
            if failure.remaining <= 1 {
                let error = failure.error;
                self.failure = None;
                return Err(error.into());
            }
            failure.remaining -= 1;
        }
        let _ = operation;
        Ok(())
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn bound_refusal(message: &mut String) {
    const MAX_BYTES: usize = 1024;
    if message.len() <= MAX_BYTES {
        return;
    }
    let mut end = MAX_BYTES - 3;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push_str("...");
}

fn session_id(name: &OsStr) -> Option<&str> {
    let text = name.to_str()?;
    (text.len() == 32
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(text)
}

fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn child_directory(parent: &File, name: &OsStr) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::ResolveFlags;
        Ok(fs::openat2(
            parent,
            name,
            flags() | OFlags::NOATIME,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?
        .into())
    }
    #[cfg(target_os = "macos")]
    {
        Ok(fs::openat(parent, name, flags(), Mode::empty())?.into())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (parent, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "records storage supports Linux; macOS is for development",
        ))
    }
}

fn record_node(parent: &File, name: &OsStr) -> io::Result<Node> {
    let node = Node::at(parent, name)?;
    #[cfg(target_os = "linux")]
    {
        // O_PATH inspects even a special inode without opening its device or
        // data stream. The caller rejects every type except a regular file.
        use rustix::fs::ResolveFlags;
        let file: File = fs::openat2(
            parent,
            name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?
        .into();
        if Node::of(&file)? != node {
            return Err(invalid("record changed while inspecting it"));
        }
    }
    Ok(node)
}

fn require_link(parent: &File, name: &OsStr, expected: &Node) -> io::Result<()> {
    let current = Node::at(parent, name)?;
    if !current.same_object(expected)
        || (current.mode, current.uid, current.gid) != (expected.mode, expected.uid, expected.gid)
    {
        return Err(invalid("directory link changed during removal"));
    }
    Ok(())
}

fn names(directory: &File, limit: usize) -> io::Result<Vec<OsString>> {
    let mut result = Vec::new();
    for entry in fs::Dir::read_from(directory)? {
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if result.len() == limit {
            return Err(invalid("directory entry limit exceeded"));
        }
        result.push(OsStr::from_bytes(bytes).to_owned());
    }
    result.sort();
    Ok(result)
}

fn trusted_root(path: &Path, owner: u32) -> io::Result<(File, OsString, File)> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > 4096 {
        return Err(invalid("root must be a bounded absolute path"));
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => components.push(name),
            _ => {
                return Err(invalid(
                    "root must not contain parent or relative components",
                ));
            }
        }
    }
    let Some((name, ancestors)) = components.split_last() else {
        return Err(invalid("filesystem root is not a records root"));
    };
    let mut parent: File = fs::open("/", flags(), Mode::empty())?.into();
    trusted_ancestor(&Node::of(&parent)?, owner)?;
    for component in ancestors {
        parent = fs::openat(&parent, *component, flags(), Mode::empty())?.into();
        trusted_ancestor(&Node::of(&parent)?, owner)?;
    }
    let root: File = fs::openat(&parent, *name, flags(), Mode::empty())?.into();
    let node = Node::of(&root)?;
    node.directory(owner, node.dev)?;
    // This second opening supplies NOATIME and verifies the Linux primitives;
    // opening a directory without enumerating it does not read its entries.
    let root = child_directory(&root, OsStr::new("."))?;
    Ok((parent, (*name).to_owned(), root))
}

fn trusted_ancestor(node: &Node, owner: u32) -> io::Result<()> {
    let sticky_root = node.uid == 0 && node.mode & 0o1000 != 0;
    if node.kind != FileType::Directory
        || (node.uid != 0 && node.uid != i128::from(owner))
        || (node.mode & 0o022 != 0 && !sticky_root)
    {
        return Err(invalid(
            "root ancestor is not owned and protected from other writers",
        ));
    }
    Ok(())
}

#[cfg(test)]
struct Failure {
    operation: Mutation,
    remaining: usize,
    error: rustix::io::Errno,
}

#[cfg(test)]
#[path = "../tests/support/openat2.rs"]
mod test_openat2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self as stdfs, FileTimes, OpenOptions, Permissions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    use super::test_openat2::store_cannot_run_here;

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            // /tmp itself is a symlink on macOS. Public roots must use the
            // actual path, because every component is opened with NOFOLLOW.
            #[cfg(target_os = "macos")]
            let temporary = Path::new("/private/tmp");
            #[cfg(not(target_os = "macos"))]
            let temporary = Path::new("/tmp");
            let base = temporary.join(format!(
                "lamp-records-store-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            private_dir(&base);
            let root = base.join("root");
            private_dir(&root);
            Self { base, root }
        }

        fn store(&self) -> Store {
            let parent: File = fs::open(&self.base, flags(), Mode::empty()).unwrap().into();
            let root = child_directory(&parent, OsStr::new("root")).unwrap();
            Store::from_open(
                parent,
                "root".into(),
                root,
                None,
                rustix::process::geteuid().as_raw(),
            )
            .unwrap()
        }

        fn session(&self, number: u64, files: usize) -> String {
            let id = format!("{number:032x}");
            let path = self.root.join(&id);
            private_dir(&path);
            private_dir(&path.join("metadata"));
            for name in &RECORDS[..files] {
                private_file(&path.join("metadata").join(name));
            }
            id
        }

        fn record(&self, id: &str, name: &str) -> PathBuf {
            self.root.join(id).join("metadata").join(name)
        }

        fn quarantine(&self, id: &str) -> PathBuf {
            self.root.join(TRASH).join(id)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // This is only the fresh, private test fixture, never a configured
            // production root. Symlink sentinels also remain inside this tree.
            stdfs::remove_dir_all(&self.base).unwrap();
        }
    }

    fn private_dir(path: &Path) {
        stdfs::create_dir(path).unwrap();
        stdfs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
    }

    fn private_file(path: &Path) {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(&[b'x'; 8192]).unwrap();
        file.sync_all().unwrap();
    }

    fn node(path: &Path) -> Node {
        Node::from_stat(fs::stat(path).unwrap())
    }

    fn fail(store: &mut Store, operation: Mutation, remaining: usize, error: rustix::io::Errno) {
        store.failure = Some(Failure {
            operation,
            remaining,
            error,
        });
    }

    /// A voice session folder as earlier builds of the voice wrote it
    /// (`docs/updates.md`, "Every part's saved data, one version ahead",
    /// `tests/fixtures/stores/README.md`): kept and counted, never refused,
    /// so a put-back lamp-records neither stops on a newer voice's sessions
    /// nor leaves them unpruned. The sessions are in `/var/lib`, so a
    /// restart in the same boot and one after a reboot see the same
    /// folders: both are this inventory.
    #[test]
    fn each_kept_session_folder_is_taken_as_a_session() {
        if store_cannot_run_here() {
            return;
        }
        let kept = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stores");
        for day in ["2026-10-22"] {
            let fixture = Fixture::new();
            let sessions = kept.join(format!("session-{day}"));
            let mut ids = Vec::new();
            for session in stdfs::read_dir(&sessions).unwrap() {
                let id = session.unwrap().file_name().into_string().unwrap();
                // Git keeps no modes: made private as the voice makes them.
                private_dir(&fixture.root.join(&id));
                private_dir(&fixture.root.join(&id).join("metadata"));
                for record in stdfs::read_dir(sessions.join(&id).join("metadata")).unwrap() {
                    let name = record.unwrap().file_name();
                    let mut file = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(fixture.record(&id, name.to_str().unwrap()))
                        .unwrap();
                    file.write_all(
                        &stdfs::read(sessions.join(&id).join("metadata").join(&name)).unwrap(),
                    )
                    .unwrap();
                }
                ids.push(id);
            }
            let inventory = fixture.store().inventory().unwrap();
            assert!(
                inventory.refused.is_empty(),
                "{day}: {:?}",
                inventory.refused
            );
            assert_eq!(
                inventory
                    .sessions
                    .iter()
                    .map(|session| session.id.clone())
                    .collect::<Vec<_>>(),
                ids,
                "{day}"
            );
        }
    }

    #[test]
    fn inventory_is_read_only_and_uses_allocated_bytes_and_newest_mtime() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut paths = vec![
            fixture.root.join(&id),
            fixture.root.join(&id).join("metadata"),
        ];
        paths.extend(RECORDS.map(|name| fixture.record(&id, name)));
        for path in &paths {
            File::open(path)
                .unwrap()
                .set_times(
                    FileTimes::new()
                        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000)),
                )
                .unwrap();
        }
        File::open(paths.last().unwrap())
            .unwrap()
            .set_times(
                FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(2000)),
            )
            .unwrap();
        paths.push(fixture.root.clone());
        let before: Vec<_> = paths.iter().map(|path| node(path)).collect();
        let mut store = fixture.store();
        let inventory = store.inventory().unwrap();
        assert!(inventory.refused.is_empty());
        assert_eq!(inventory.sessions.len(), 1);
        assert_eq!(inventory.sessions[0].id, id);
        assert_eq!(inventory.sessions[0].newest_unix_s, 2000);
        assert!(!inventory.sessions[0].pending);
        assert_eq!(
            inventory.sessions[0].bytes,
            before[..7]
                .iter()
                .map(|node| node.bytes().unwrap())
                .sum::<u64>()
        );
        assert_eq!(
            before,
            paths.iter().map(|path| node(path)).collect::<Vec<_>>()
        );
        assert!(!fixture.root.join(TRASH).exists());
        assert_eq!(
            stdfs::read(fixture.record(&id, RECORDS[0])).unwrap(),
            vec![b'x'; 8192]
        );
        let disk = store.disk().unwrap();
        assert!(disk.total_bytes > 0 && disk.available_bytes <= disk.total_bytes);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_inventory_preserves_directory_and_record_atime() {
        if store_cannot_run_here() {
            return;
        }
        use std::os::unix::fs::MetadataExt;
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut paths = vec![
            fixture.root.clone(),
            fixture.root.join(&id),
            fixture.root.join(&id).join("metadata"),
        ];
        paths.extend(RECORDS.map(|name| fixture.record(&id, name)));
        for path in &paths {
            File::open(path)
                .unwrap()
                .set_times(
                    FileTimes::new()
                        .set_accessed(SystemTime::UNIX_EPOCH + Duration::from_secs(1000)),
                )
                .unwrap();
        }
        let before: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = stdfs::symlink_metadata(path).unwrap();
                (metadata.atime(), metadata.atime_nsec())
            })
            .collect();
        fixture.store().inventory().unwrap();
        let after: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = stdfs::symlink_metadata(path).unwrap();
                (metadata.atime(), metadata.atime_nsec())
            })
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn removes_only_inventoried_sessions_and_requires_fresh_inventory_for_retry() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut store = fixture.store();
        assert!(store.remove(&id).is_err());
        let inventory = store.inventory().unwrap();
        let later = fixture.session(2, 1);
        assert!(store.remove(&later).is_err());
        let removed = store.remove(&id).unwrap();
        assert!(removed.complete && removed.error.is_none());
        assert_eq!(removed.removed_files, 7);
        assert!(removed.removed_bytes > 0 && removed.removed_bytes <= inventory.sessions[0].bytes);
        assert!(!fixture.root.join(&id).exists());
        assert!(!fixture.quarantine(&id).exists());
        assert!(fixture.root.join(later).exists());
        assert!(store.remove(&id).is_err());
    }

    #[test]
    fn partial_start_with_empty_metadata_is_known_but_empty_live_session_is_not() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 0);
        let empty_id = format!("{:032x}", 2);
        private_dir(&fixture.root.join(&empty_id));
        let mut store = fixture.store();
        let inventory = store.inventory().unwrap();
        assert_eq!(inventory.sessions.len(), 1);
        assert_eq!(inventory.sessions[0].id, id);
        assert_eq!(inventory.refused.len(), 1);
        assert!(store.remove(&empty_id).is_err());
        assert_eq!(store.remove(&id).unwrap().removed_files, 2);
    }

    #[test]
    fn links_specials_unknown_layouts_and_permissions_are_refused_without_reads() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let outside = fixture.base.join("outside");
        private_dir(&outside);
        private_file(&outside.join("sentinel"));

        let top = format!("{:032x}", 1);
        symlink(&outside, fixture.root.join(&top)).unwrap();
        let metadata = format!("{:032x}", 2);
        private_dir(&fixture.root.join(&metadata));
        symlink(&outside, fixture.root.join(&metadata).join("metadata")).unwrap();
        let file = fixture.session(3, 0);
        symlink(outside.join("sentinel"), fixture.record(&file, RECORDS[0])).unwrap();
        let hard = fixture.session(4, 0);
        stdfs::hard_link(outside.join("sentinel"), fixture.record(&hard, RECORDS[0])).unwrap();
        let special = fixture.session(5, 0);
        private_dir(&fixture.record(&special, RECORDS[0]));
        let unknown = fixture.session(6, 1);
        private_file(&fixture.record(&unknown, "other.jsonl"));
        let old_layout = fixture.session(7, 1);
        private_file(&fixture.root.join(&old_layout).join("events.jsonl"));
        let mode = fixture.session(8, 1);
        stdfs::set_permissions(
            fixture.record(&mode, RECORDS[0]),
            Permissions::from_mode(0o644),
        )
        .unwrap();
        let directory_mode = fixture.session(9, 0);
        stdfs::set_permissions(
            fixture.root.join(&directory_mode).join("metadata"),
            Permissions::from_mode(0o750),
        )
        .unwrap();
        private_dir(&fixture.root.join("not-a-session"));

        let mut store = fixture.store();
        let inventory = store.inventory().unwrap();
        assert!(inventory.sessions.is_empty());
        assert_eq!(inventory.refused.len(), 10);
        for id in [
            &top,
            &metadata,
            &file,
            &hard,
            &special,
            &unknown,
            &old_layout,
            &mode,
            &directory_mode,
        ] {
            assert!(store.remove(id).is_err());
            assert!(stdfs::symlink_metadata(fixture.root.join(id)).is_ok());
        }
        assert!(!fixture.root.join(TRASH).exists());
        assert_eq!(
            stdfs::read(outside.join("sentinel")).unwrap(),
            vec![b'x'; 8192]
        );
    }

    #[test]
    fn a_mutated_record_invalidates_the_whole_candidate_before_any_unlink() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut store = fixture.store();
        store.inventory().unwrap();
        OpenOptions::new()
            .append(true)
            .open(fixture.record(&id, RECORDS[4]))
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert!(store.remove(&id).is_err());
        assert!(!fixture.root.join(TRASH).exists());
        assert_eq!(
            stdfs::read_dir(fixture.root.join(&id).join("metadata"))
                .unwrap()
                .count(),
            5
        );
        assert!(store.remove(&id).is_err());
        store.inventory().unwrap();
        assert!(store.remove(&id).unwrap().complete);
    }

    #[test]
    fn changed_file_inode_even_with_same_size_and_mtime_is_refused() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let path = fixture.record(&id, RECORDS[4]);
        let times = stdfs::metadata(&path).unwrap().modified().unwrap();
        let mut store = fixture.store();
        store.inventory().unwrap();
        stdfs::rename(&path, fixture.base.join("old-file")).unwrap();
        private_file(&path);
        File::open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(times))
            .unwrap();
        assert!(store.remove(&id).is_err());
        assert!(!fixture.root.join(TRASH).exists());
        assert!(path.exists() && fixture.base.join("old-file").exists());
    }

    #[test]
    fn vanished_candidate_is_not_recreated_or_confused_with_a_replacement() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut store = fixture.store();
        store.inventory().unwrap();
        stdfs::rename(fixture.root.join(&id), fixture.base.join("moved")).unwrap();
        assert_eq!(
            store.remove(&id).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!fixture.root.join(&id).exists());
        let replaced = fixture.session(1, 5);
        assert_eq!(replaced, id);
        assert!(store.remove(&id).is_err());
        assert!(fixture.root.join(&id).exists());
        assert_eq!(
            stdfs::read(fixture.base.join("moved/metadata/health.json")).unwrap(),
            vec![b'x'; 8192]
        );
    }

    #[test]
    fn replaced_session_and_metadata_directory_keep_both_trees_intact() {
        if store_cannot_run_here() {
            return;
        }
        for replace_metadata in [false, true] {
            let fixture = Fixture::new();
            let id = fixture.session(1, 5);
            let mut store = fixture.store();
            store.inventory().unwrap();
            let target = if replace_metadata {
                fixture.root.join(&id).join("metadata")
            } else {
                fixture.root.join(&id)
            };
            stdfs::rename(&target, fixture.base.join("moved")).unwrap();
            if replace_metadata {
                private_dir(&target);
                for name in RECORDS {
                    private_file(&target.join(name));
                }
            } else {
                fixture.session(1, 5);
            }
            assert!(store.remove(&id).is_err());
            assert!(target.exists() && fixture.base.join("moved").exists());
            assert!(!fixture.root.join(TRASH).exists());
        }
    }

    #[test]
    fn moved_or_replaced_root_is_detected_before_mutation() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut store = fixture.store();
        store.inventory().unwrap();
        stdfs::rename(&fixture.root, fixture.base.join("moved-root")).unwrap();
        private_dir(&fixture.root);
        fixture.session(1, 5);
        assert!(store.remove(&id).is_err());
        assert!(store.disk().is_err());
        assert!(store.inventory().is_err());
        assert!(fixture.root.join(&id).exists());
        assert!(fixture.base.join("moved-root").join(&id).exists());
    }

    #[test]
    fn cleaner_lock_is_nonblocking_and_does_not_create_a_file() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let first = fixture.store();
        let parent: File = fs::open(&fixture.base, flags(), Mode::empty())
            .unwrap()
            .into();
        let root = child_directory(&parent, OsStr::new("root")).unwrap();
        let second = Store::from_open(
            parent,
            "root".into(),
            root,
            None,
            rustix::process::geteuid().as_raw(),
        );
        assert!(second.is_err());
        assert_eq!(stdfs::read_dir(&fixture.root).unwrap().count(), 0);
        drop(first);
        assert!(fixture.store().inventory().unwrap().sessions.is_empty());
    }

    #[test]
    fn interrupted_unlinks_report_actual_progress_and_every_remainder_recovers() {
        if store_cannot_run_here() {
            return;
        }
        for stop_before in 1..=7 {
            let fixture = Fixture::new();
            let id = fixture.session(1, 5);
            let first_file_bytes = node(&fixture.record(&id, RECORDS[0])).bytes().unwrap();
            let mut store = fixture.store();
            store.inventory().unwrap();
            fail(
                &mut store,
                Mutation::Unlink,
                stop_before,
                rustix::io::Errno::IO,
            );
            let result = store.remove(&id);
            if stop_before == 1 {
                assert!(result.is_err());
            } else {
                let result = result.unwrap();
                assert!(!result.complete && result.error.is_some());
                assert_eq!(result.removed_files, (stop_before - 1) as u64);
                assert!(result.removed_bytes >= first_file_bytes);
            }
            assert!(!fixture.root.join(&id).exists());
            assert!(fixture.quarantine(&id).exists());
            drop(store);
            let mut restarted = fixture.store();
            let inventory = restarted.inventory().unwrap();
            assert!(inventory.refused.is_empty());
            assert_eq!(inventory.sessions.len(), 1);
            assert!(inventory.sessions[0].pending);
            let remaining = restarted.remove(&id).unwrap();
            assert!(remaining.complete);
            assert_eq!(remaining.removed_files, (8 - stop_before) as u64);
            assert!(!fixture.quarantine(&id).exists());
        }
    }

    #[test]
    fn full_disk_before_mutation_never_falls_back_to_in_place_deletion() {
        if store_cannot_run_here() {
            return;
        }
        for operation in [Mutation::Mkdir, Mutation::Rename, Mutation::Sync] {
            let fixture = Fixture::new();
            let id = fixture.session(1, 5);
            let mut store = fixture.store();
            store.inventory().unwrap();
            fail(&mut store, operation, 1, rustix::io::Errno::NOSPC);
            let error = store.remove(&id).unwrap_err();
            assert_eq!(
                error.raw_os_error(),
                Some(rustix::io::Errno::NOSPC.raw_os_error())
            );
            for name in RECORDS {
                assert!(fixture.record(&id, name).exists());
            }
            assert!(!fixture.quarantine(&id).exists());
        }
    }

    #[test]
    fn fsync_failures_keep_truthful_counts_and_a_recognizable_remainder() {
        if store_cannot_run_here() {
            return;
        }
        // New quarantine sync, rename source sync, rename destination sync,
        // metadata sync after files, session sync after metadata, final sync.
        for stop_before in 1..=6 {
            let fixture = Fixture::new();
            let id = fixture.session(1, 5);
            let mut store = fixture.store();
            store.inventory().unwrap();
            fail(
                &mut store,
                Mutation::Sync,
                stop_before,
                rustix::io::Errno::NOSPC,
            );
            let result = store.remove(&id);
            if stop_before <= 3 {
                assert!(result.is_err());
            } else {
                let result = result.unwrap();
                assert!(!result.complete && result.error.is_some());
                assert_eq!(result.removed_files, (stop_before + 1) as u64);
            }
            drop(store);
            let mut restarted = fixture.store();
            let inventory = restarted.inventory().unwrap();
            assert!(inventory.refused.is_empty());
            if stop_before == 6 {
                assert!(inventory.sessions.is_empty());
            } else {
                assert_eq!(inventory.sessions.len(), 1);
                assert!(restarted.remove(&id).unwrap().complete);
            }
        }
    }

    #[test]
    fn conflicting_quarantine_id_never_overwrites_and_duplicate_inventory_refuses_both() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let mut store = fixture.store();
        store.inventory().unwrap();
        private_dir(&fixture.root.join(TRASH));
        private_dir(&fixture.quarantine(&id));
        assert!(store.remove(&id).is_err());
        assert!(fixture.root.join(&id).exists() && fixture.quarantine(&id).exists());
        let inventory = store.inventory().unwrap();
        assert!(inventory.sessions.is_empty());
        assert!(
            inventory
                .refused
                .iter()
                .any(|error| error.contains("both live and quarantine"))
        );
        assert!(store.remove(&id).is_err());
    }

    #[test]
    fn unknown_or_linked_quarantine_is_preserved() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 5);
        let outside = fixture.base.join("outside");
        private_dir(&outside);
        private_file(&outside.join("sentinel"));
        symlink(&outside, fixture.root.join(TRASH)).unwrap();
        let mut store = fixture.store();
        let inventory = store.inventory().unwrap();
        assert_eq!(inventory.sessions.len(), 1);
        assert_eq!(inventory.refused.len(), 1);
        assert!(store.remove(&id).is_err());
        assert!(fixture.root.join(&id).exists());
        assert!(outside.join("sentinel").exists());
    }

    #[test]
    fn traversal_ids_invalid_roots_and_bounded_enumeration_fail_closed() {
        if store_cannot_run_here() {
            return;
        }
        for name in [
            "..",
            "../outside",
            "/outside",
            "",
            "ABCDEF0123456789abcdef0123456789",
            "0000000000000000000000000000000g",
        ] {
            assert!(session_id(OsStr::new(name)).is_none());
        }
        for root in ["/", "relative/path", "/var/lib/../outside"] {
            assert!(Store::open(Path::new(root)).is_err());
        }
        let fixture = Fixture::new();
        fixture.session(1, 0);
        let mut store = fixture.store();
        assert!(names(&store.root, 0).is_err());
        assert_eq!(names(&store.root, 1).unwrap().len(), 1);
        store.inventory().unwrap();
        assert!(store.remove("../outside").is_err());
    }

    #[test]
    fn accounting_rejects_overflow_and_snapshots_include_ctime_but_not_atime() {
        let fixture = Fixture::new();
        let id = fixture.session(1, 1);
        let original = node(&fixture.record(&id, RECORDS[0]));
        let mut changed = original.clone();
        changed.ctime.1 += 1;
        assert_ne!(original, changed);
        changed = original;
        changed.blocks = i128::from(u64::MAX);
        assert!(changed.bytes().is_err());
        changed.blocks = -1;
        assert!(changed.bytes().is_err());
        changed.mtime.0 = -1;
        assert!(changed.unix_s().is_err());
    }

    #[test]
    fn combined_live_and_quarantine_entry_budget_is_enforced_and_clears_prior_authority() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let live = fixture.session(1, 0);
        fixture.session(2, 0);
        let pending = fixture.session(3, 0);
        private_dir(&fixture.root.join(TRASH));
        stdfs::rename(fixture.root.join(&pending), fixture.quarantine(&pending)).unwrap();
        let mut store = fixture.store();
        let inventory = store.inventory_bounded(3).unwrap();
        assert_eq!(inventory.sessions.len(), 3);
        assert!(
            inventory
                .sessions
                .iter()
                .find(|item| item.id == pending)
                .unwrap()
                .pending
        );
        assert!(
            !inventory
                .sessions
                .iter()
                .find(|item| item.id == live)
                .unwrap()
                .pending
        );
        let extra = fixture.session(4, 0);
        stdfs::rename(fixture.root.join(&extra), fixture.quarantine(&extra)).unwrap();
        assert!(store.inventory_bounded(3).is_err());
        assert!(store.remove(&live).is_err());
        assert!(fixture.root.join(live).exists());
        assert!(fixture.quarantine(&pending).exists());
        assert!(fixture.quarantine(&extra).exists());
        // Unknown names also consume work budget; they cannot allow unbounded
        // scans merely because they are excluded from eligible sessions.
        private_dir(&fixture.root.join("unknown"));
        assert!(store.inventory_bounded(4).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifo_is_refused_without_opening_a_blocking_stream() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 0);
        let metadata: File = fs::open(
            fixture.root.join(&id).join("metadata"),
            flags(),
            Mode::empty(),
        )
        .unwrap()
        .into();
        fs::mknodat(
            &metadata,
            RECORDS[0],
            FileType::Fifo,
            Mode::from_bits_truncate(0o600),
            0,
        )
        .unwrap();
        let mut store = fixture.store();
        let inventory = store.inventory().unwrap();
        assert!(inventory.sessions.is_empty());
        assert_eq!(inventory.refused.len(), 1);
        assert!(store.remove(&id).is_err());
        assert_eq!(
            Node::at(&metadata, OsStr::new(RECORDS[0])).unwrap().kind,
            FileType::Fifo
        );
    }

    #[test]
    fn sparse_file_allocation_is_not_confused_with_logical_length() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 1);
        let path = fixture.record(&id, RECORDS[0]);
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(1 << 30)
            .unwrap();
        let record = node(&path);
        assert!(record.bytes().unwrap() < 1 << 30);
        let inventory = fixture.store().inventory().unwrap();
        let directory_bytes = node(&fixture.root.join(&id)).bytes().unwrap()
            + node(&fixture.root.join(&id).join("metadata"))
                .bytes()
                .unwrap();
        assert_eq!(
            inventory.sessions[0].bytes,
            record.bytes().unwrap() + directory_bytes
        );
    }

    #[test]
    fn refusal_limit_is_utf8_bytes_including_escaped_name_and_prefix() {
        let bytes = vec![0xff; 255];
        let mut escaped = format!("{:?}: unknown entry name", OsStr::from_bytes(&bytes));
        bound_refusal(&mut escaped);
        assert!(escaped.len() <= 1024);
        assert!(escaped.ends_with("..."));
        let mut unicode = "💡".repeat(300);
        bound_refusal(&mut unicode);
        assert!(unicode.len() <= 1024 && unicode.ends_with("..."));
        let mut short = "session changed".to_owned();
        bound_refusal(&mut short);
        assert_eq!(short, "session changed");
    }

    #[test]
    fn public_constructor_accepts_a_private_tree_below_root_owned_sticky_temp() {
        if store_cannot_run_here() {
            return;
        }
        let fixture = Fixture::new();
        let id = fixture.session(1, 1);
        let mut store = Store::open(&fixture.root).unwrap();
        assert_eq!(store.inventory().unwrap().sessions[0].id, id);
        assert!(store.disk().unwrap().total_bytes > 0);
        assert!(store.remove(&id).unwrap().complete);
        assert!(!fixture.root.join(id).exists());
    }

    #[test]
    fn public_constructor_rejects_writable_nonsticky_ancestors_open_root_and_symlinks() {
        let fixture = Fixture::new();
        stdfs::set_permissions(&fixture.base, Permissions::from_mode(0o777)).unwrap();
        assert!(Store::open(&fixture.root).is_err());
        if rustix::process::geteuid().as_raw() != 0 {
            stdfs::set_permissions(&fixture.base, Permissions::from_mode(0o1777)).unwrap();
            assert!(Store::open(&fixture.root).is_err());
        }
        stdfs::set_permissions(&fixture.base, Permissions::from_mode(0o700)).unwrap();
        stdfs::set_permissions(&fixture.root, Permissions::from_mode(0o755)).unwrap();
        assert!(Store::open(&fixture.root).is_err());
        stdfs::set_permissions(&fixture.root, Permissions::from_mode(0o700)).unwrap();
        private_dir(&fixture.root.join("nested"));
        let alias = fixture.base.join("alias");
        symlink(&fixture.root, &alias).unwrap();
        assert!(Store::open(&alias).is_err());
        assert!(Store::open(&alias.join("nested")).is_err());
        assert!(Store::open(Path::new("/")).is_err());
    }

    #[test]
    fn sticky_exception_requires_root_ownership_and_never_relaxes_final_root() {
        let fixture = Fixture::new();
        let mut candidate = node(&fixture.root);
        candidate.uid = 1234;
        candidate.mode = 0o41777;
        assert!(trusted_ancestor(&candidate, 1234).is_err());
        candidate.uid = 0;
        assert!(trusted_ancestor(&candidate, 1234).is_ok());
        assert!(candidate.directory(0, candidate.dev).is_err());
        candidate.mode = 0o40777;
        assert!(trusted_ancestor(&candidate, 1234).is_err());
        candidate.mode = 0o40755;
        assert!(trusted_ancestor(&candidate, 1234).is_ok());
        candidate.uid = 5678;
        assert!(trusted_ancestor(&candidate, 1234).is_err());
    }

    #[test]
    fn invalid_quarantine_shares_the_total_entry_budget_and_clears_prior_authority() {
        if store_cannot_run_here() {
            return;
        }
        for unknown_names in [false, true] {
            let fixture = Fixture::new();
            let ids: Vec<_> = (1..=3).map(|number| fixture.session(number, 0)).collect();
            let mut store = fixture.store();
            assert_eq!(store.inventory_bounded(3).unwrap().sessions.len(), 3);
            assert_eq!(store.snapshots.len(), 3);
            if unknown_names {
                for (index, id) in ids.iter().enumerate() {
                    stdfs::rename(
                        fixture.root.join(id),
                        fixture.root.join(format!("unknown-{index}")),
                    )
                    .unwrap();
                }
            }
            let outside = fixture.base.join("outside");
            private_dir(&outside);
            private_file(&outside.join("sentinel"));
            symlink(&outside, fixture.root.join(TRASH)).unwrap();
            let error = store.inventory_bounded(3).unwrap_err();
            assert!(error.to_string().contains("quarantine exceeds"));
            assert!(store.snapshots.is_empty());
            for id in &ids {
                assert!(store.remove(id).is_err());
            }
            assert_eq!(stdfs::read_dir(&fixture.root).unwrap().count(), 4);
            assert_eq!(
                stdfs::read(outside.join("sentinel")).unwrap(),
                vec![b'x'; 8192]
            );
        }
    }
}
