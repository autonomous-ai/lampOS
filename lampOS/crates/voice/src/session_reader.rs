//! Opt-in, startup-only sharing of newly created service metadata. This never
//! changes an ancestor or old session, and holds no conversation authority.
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

pub(crate) const ENVIRONMENT: &str = "LAMP_SESSION_READ_GROUP";
const MAX_GROUP_BYTES: u64 = 1024 * 1024;

/// Read the explicit setting only in continuous startup, before workers. No
/// lookup occurs for a finite run or an unset setting. Errors degrade metadata.
pub(crate) fn startup(continuous: bool, output: &Path) -> Option<io::Result<Reader>> {
    requested(continuous, output, || std::env::var_os(ENVIRONMENT), group)
}
fn requested(
    continuous: bool,
    output: &Path,
    setting: impl FnOnce() -> Option<OsString>,
    lookup: impl FnOnce(&str) -> io::Result<u32>,
) -> Option<io::Result<Reader>> {
    if !continuous {
        return None;
    }
    setting().map(|name| {
        let name = name
            .to_str()
            .ok_or_else(|| invalid("invalid reader group"))?;
        Reader::prepare(output, lookup(name)?)
    })
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn group(name: &str) -> io::Result<u32> {
    validate_name(name)?;
    let file: File = fs::open(
        "/etc/group",
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != 0 || meta.nlink() != 1 || meta.mode() & 0o022 != 0 {
        return Err(invalid("untrusted local group file"));
    }
    group_from(name, file)
}
fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(invalid("invalid reader group"));
    }
    Ok(())
}
fn group_from(name: &str, source: impl Read) -> io::Result<u32> {
    validate_name(name)?;
    let mut bytes = Vec::new();
    source.take(MAX_GROUP_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_GROUP_BYTES {
        return Err(invalid("local group file exceeds bound"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("invalid local group file"))?;
    let mut found = None;
    for line in text.lines() {
        let mut fields = line.split(':');
        if fields.next() != Some(name) {
            continue;
        }
        let _password = fields
            .next()
            .ok_or_else(|| invalid("invalid local group entry"))?;
        let gid = fields
            .next()
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|gid| *gid != u32::MAX)
            .ok_or_else(|| invalid("invalid local group id"))?;
        if fields.next().is_none() || fields.next().is_some() || found.replace(gid).is_some() {
            return Err(invalid("reader group is not unique"));
        }
    }
    found.ok_or_else(|| invalid("reader group is absent"))
}

#[cfg(all(test, target_os = "linux"))]
#[derive(Clone, Debug, Default)]
struct FailureObservation {
    injected_grant_hit: bool,
    injected_rollback_hit: bool,
    apply_error: Option<String>,
}

pub(crate) struct Reader {
    gid: u32,
    voice: File,
    root: File,
    root_path: PathBuf,
    root_name: OsString,
    session: OsString,
    #[cfg(test)]
    failure: Option<(usize, Option<usize>)>,
    #[cfg(all(test, target_os = "linux"))]
    failure_observation: Option<std::sync::Arc<std::sync::Mutex<FailureObservation>>>,
}
impl Reader {
    pub(super) fn prepare(output: &Path, gid: u32) -> io::Result<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (output, gid);
            Err(invalid("session reader verification requires Linux"))
        }
        #[cfg(target_os = "linux")]
        {
            if gid == u32::MAX {
                return Err(invalid("invalid reader group id"));
            }
            let root_path = output.parent().ok_or_else(|| invalid("no sessions root"))?;
            let voice_path = root_path
                .parent()
                .ok_or_else(|| invalid("no voice parent"))?;
            let session = output
                .file_name()
                .ok_or_else(|| invalid("no session name"))?
                .to_owned();
            let root_name = root_path
                .file_name()
                .ok_or_else(|| invalid("no sessions name"))?
                .to_owned();
            let voice = trusted_directory(voice_path)?;
            directory(&voice, 0o710, Some(gid), None)?;
            let root = child(&voice, &root_name)?;
            directory(&root, 0o750, Some(gid), Some(voice.metadata()?.dev()))?;
            private_siblings(&voice, &root_name)?;
            match fs::statat(&root, &session, AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => {}
                _ => return Err(invalid("session reader requires a new session")),
            }
            Ok(Self {
                gid,
                voice,
                root,
                root_path: root_path.to_owned(),
                root_name,
                session,
                #[cfg(test)]
                failure: None,
                #[cfg(test)]
                failure_observation: None,
            })
        }
    }

    fn parents(&self) -> io::Result<()> {
        directory(&self.voice, 0o710, Some(self.gid), None)?;
        private_siblings(&self.voice, &self.root_name)?;
        directory(
            &self.root,
            0o750,
            Some(self.gid),
            Some(self.voice.metadata()?.dev()),
        )?;
        let current = child(&self.voice, &self.root_name)?;
        same(&current, &self.root)?;
        let voice_path = self
            .root_path
            .parent()
            .ok_or_else(|| invalid("no voice parent"))?;
        same(&trusted_directory(voice_path)?, &self.voice)
    }

    /// All objects already exist privately and exclusively, before the writer
    /// starts. Rotation retains their descriptors and never returns here.
    /// Ok(false) means a verified private rollback; Err disables the writer.
    pub(crate) fn grant(self, output: &Path, events: &[File], health: &File) -> io::Result<bool> {
        #[cfg(test)]
        let failure = self.failure;
        #[cfg(all(test, target_os = "linux"))]
        let observation = self.failure_observation.clone();
        self.grant_with(output, events, health, move |rollback, step| {
            #[cfg(test)]
            if failure.is_some_and(|(grant, back)| {
                if rollback {
                    back == Some(step)
                } else {
                    grant == step
                }
            }) {
                #[cfg(target_os = "linux")]
                if let Some(observation) = &observation {
                    let mut observation = observation.lock().unwrap();
                    if rollback {
                        observation.injected_rollback_hit = true;
                    } else {
                        observation.injected_grant_hit = true;
                    }
                }
                return Err(io::Error::other("injected reader grant failure"));
            }
            let _ = (rollback, step);
            Ok(())
        })
    }

    fn grant_with(
        self,
        output: &Path,
        events: &[File],
        health: &File,
        mut before: impl FnMut(bool, usize) -> io::Result<()>,
    ) -> io::Result<bool> {
        if output.parent() != Some(self.root_path.as_path())
            || output.file_name() != Some(self.session.as_os_str())
        {
            return Err(invalid("session path changed"));
        }
        let session = child(&self.root, &self.session)?;
        let metadata = child(&session, OsStr::new("metadata"))?;
        let originals: Vec<_> = std::iter::once(&session)
            .chain(std::iter::once(&metadata))
            .chain(events)
            .map(|file| file.metadata().map(|meta| meta.gid()))
            .collect::<io::Result<_>>()?;
        let apply = (|| {
            self.parents()?;
            verify(
                &self.root,
                &self.session,
                &session,
                &metadata,
                events,
                health,
                None,
            )?;
            let mut step = 0;
            let mut next = || {
                let result = before(false, step);
                step += 1;
                result
            };
            for file in events {
                next()?;
                fs::fchown(file, None, Some(rustix::process::Gid::from_raw(self.gid)))?;
                next()?;
                fs::fchmod(file, Mode::from_raw_mode(0o640))?;
            }
            next()?;
            fs::fchown(
                &metadata,
                None,
                Some(rustix::process::Gid::from_raw(self.gid)),
            )?;
            next()?;
            fs::fchmod(&metadata, Mode::from_raw_mode(0o750))?;
            next()?;
            fs::fchown(
                &session,
                None,
                Some(rustix::process::Gid::from_raw(self.gid)),
            )?;
            self.parents()?;
            // Children are prepared, but the outer session still gates access.
            directory(
                &session,
                0o700,
                Some(self.gid),
                Some(self.root.metadata()?.dev()),
            )?;
            verify_children(&session, &metadata, events, health, Some(self.gid))?;
            next()?;
            fs::fchmod(&session, Mode::from_raw_mode(0o750))?;
            next()?;
            verify(
                &self.root,
                &self.session,
                &session,
                &metadata,
                events,
                health,
                Some(self.gid),
            )
        })();
        #[cfg(all(test, target_os = "linux"))]
        if self.failure.is_some()
            && let Some(observation) = &self.failure_observation
        {
            observation.lock().unwrap().apply_error = apply.as_ref().err().map(|error| {
                format!(
                    "{error} (kind={:?}, os={:?})",
                    error.kind(),
                    error.raw_os_error()
                )
            });
        }
        if apply.is_ok() {
            return Ok(true);
        }
        // Close the outer gate first, then restore every original group/mode.
        // Continue all rollback attempts; only actual final state is authority.
        let mut step = 0;
        for (index, file) in std::iter::once(&session)
            .chain(std::iter::once(&metadata))
            .chain(events)
            .enumerate()
        {
            let mode = if index < 2 { 0o700 } else { 0o600 };
            if before(true, step).is_ok() {
                let _ = fs::fchmod(file, Mode::from_raw_mode(mode));
            }
            step += 1;
            if before(true, step).is_ok() {
                let _ = fs::fchown(
                    file,
                    None,
                    Some(rustix::process::Gid::from_raw(originals[index])),
                );
            }
            step += 1;
        }
        verify(
            &self.root,
            &self.session,
            &session,
            &metadata,
            events,
            health,
            None,
        )?;
        for (index, file) in std::iter::once(&session)
            .chain(std::iter::once(&metadata))
            .chain(events)
            .enumerate()
        {
            if file.metadata()?.gid() != originals[index] {
                return Err(invalid("reader rollback incomplete"));
            }
        }
        Ok(false)
    }
}

fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
}
fn child(parent: &File, name: &OsStr) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        Ok(fs::openat2(
            parent,
            name,
            flags(),
            Mode::empty(),
            fs::ResolveFlags::BENEATH | fs::ResolveFlags::NO_SYMLINKS | fs::ResolveFlags::NO_XDEV,
        )?
        .into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (parent, name);
        Err(invalid("session reader verification requires Linux"))
    }
}
fn trusted_directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(invalid("untrusted session parent path"));
    }
    let mut file: File = fs::open("/", flags(), Mode::empty())?.into();
    #[cfg(all(test, target_os = "linux"))]
    let path = tests::fixture_root(&mut file, path)?;
    ancestor(&file)?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                file = fs::openat(&file, name, flags(), Mode::empty())?.into();
                ancestor(&file)?;
            }
            _ => return Err(invalid("untrusted session parent path")),
        }
    }
    Ok(file)
}
fn ancestor(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    let owner = rustix::process::geteuid().as_raw();
    let sticky_root = m.uid() == 0 && m.mode() & 0o1000 != 0;
    if !m.is_dir() || (m.uid() != 0 && m.uid() != owner) || (m.mode() & 0o022 != 0 && !sticky_root)
    {
        return Err(invalid("untrusted session ancestor"));
    }
    no_acl(file)
}
fn directory(file: &File, mode: u32, gid: Option<u32>, dev: Option<u64>) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_dir()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o7777 != mode
        || gid.is_some_and(|gid| m.gid() != gid)
        || dev.is_some_and(|dev| m.dev() != dev)
    {
        return Err(invalid("unprepared session directory"));
    }
    no_acl(file)
}
fn record(file: &File, mode: u32, gid: Option<u32>, dev: u64) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o7777 != mode
        || m.nlink() != 1
        || m.dev() != dev
        || gid.is_some_and(|gid| m.gid() != gid)
    {
        return Err(invalid("untrusted session record"));
    }
    no_acl(file)
}
fn no_acl(file: &File) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let before = file.metadata()?;
        for name in ["system.posix_acl_access", "system.posix_acl_default"] {
            match fs::fgetxattr(file, name, &mut [0u8; 1]) {
                Err(rustix::io::Errno::NODATA) => {}
                _ => return Err(invalid("session reader ACL cannot be verified absent")),
            }
        }
        let after = file.metadata()?;
        if (
            before.dev(),
            before.ino(),
            before.ctime(),
            before.ctime_nsec(),
        ) != (after.dev(), after.ino(), after.ctime(), after.ctime_nsec())
        {
            return Err(invalid("session object changed during ACL inspection"));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        Err(invalid("session reader verification requires Linux"))
    }
}
fn same(a: &File, b: &File) -> io::Result<()> {
    let a = a.metadata()?;
    let b = b.metadata()?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Err(invalid("session directory changed"));
    }
    Ok(())
}
fn named(parent: &File, name: &OsStr, file: &File) -> io::Result<()> {
    let node = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let m = file.metadata()?;
    if i128::from(node.st_dev) != i128::from(m.dev())
        || i128::from(node.st_ino) != i128::from(m.ino())
    {
        return Err(invalid("session record path changed"));
    }
    Ok(())
}
/// Parent search permission must not make a known sibling pathname readable.
/// This is a bounded, nonrecursive check, never a migration or chmod. External
/// setup must establish this invariant before it exposes the parent itself.
fn private_siblings(voice: &File, sessions: &OsStr) -> io::Result<()> {
    let dev = voice.metadata()?.dev();
    let mut count = 0;
    for entry in fs::Dir::read_from(voice)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        count += 1;
        if count > 128 {
            return Err(invalid("voice siblings exceed verification bound"));
        }
        if name.to_bytes() == sessions.as_encoded_bytes() {
            continue;
        }
        let name = OsStr::from_bytes(name.to_bytes());
        let node = fs::statat(voice, name, AtFlags::SYMLINK_NOFOLLOW)?;
        let file = match fs::FileType::from_raw_mode(node.st_mode) {
            fs::FileType::Directory => {
                let file = child(voice, name)?;
                directory(&file, 0o700, None, Some(dev))?;
                file
            }
            fs::FileType::RegularFile => {
                let file: File = fs::openat(
                    voice,
                    name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?
                .into();
                record(&file, 0o600, None, dev)?;
                file
            }
            _ => return Err(invalid("untrusted voice sibling")),
        };
        named(voice, name, &file)?;
    }
    Ok(())
}
fn names(file: &File, expected: &[String]) -> io::Result<()> {
    let mut names = Vec::new();
    for entry in fs::Dir::read_from(file)? {
        let entry = entry?;
        if entry.file_name().to_bytes() == b"." || entry.file_name().to_bytes() == b".." {
            continue;
        }
        if names.len() == expected.len() {
            return Err(invalid("unexpected session entries"));
        }
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    if names != expected {
        return Err(invalid("unexpected session entries"));
    }
    Ok(())
}
fn verify(
    root: &File,
    session_name: &OsStr,
    session: &File,
    metadata: &File,
    events: &[File],
    health: &File,
    gid: Option<u32>,
) -> io::Result<()> {
    directory(
        session,
        if gid.is_some() { 0o750 } else { 0o700 },
        gid,
        Some(root.metadata()?.dev()),
    )?;
    named(root, session_name, session)?;
    verify_children(session, metadata, events, health, gid)
}
fn verify_children(
    session: &File,
    metadata: &File,
    events: &[File],
    health: &File,
    gid: Option<u32>,
) -> io::Result<()> {
    let dev = session.metadata()?.dev();
    directory(
        metadata,
        if gid.is_some() { 0o750 } else { 0o700 },
        gid,
        Some(dev),
    )?;
    named(session, OsStr::new("metadata"), metadata)?;
    names(session, &["metadata".into()])?;
    let mut expected = vec!["health.json".into()];
    record(health, 0o600, None, dev)?;
    named(metadata, OsStr::new("health.json"), health)?;
    for (slot, file) in events.iter().enumerate() {
        let name = format!("events-{slot}.jsonl");
        record(file, if gid.is_some() { 0o640 } else { 0o600 }, gid, dev)?;
        named(metadata, OsStr::new(&name), file)?;
        expected.push(name);
    }
    names(metadata, &expected)
}

#[cfg(test)]
#[path = "session_reader/tests.rs"]
mod tests;
