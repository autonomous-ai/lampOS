//! Bounded configuration/result files with descriptor-relative atomic writes.
use crate::{
    RunReport,
    config::{absolute_path, invalid},
};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{File, Metadata},
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub const CONFIG_BYTES: usize = 65_536;
pub const REPORT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified_s: i64,
    modified_ns: i64,
    changed_s: i64,
    changed_ns: i64,
}

fn no_atime() -> OFlags {
    #[cfg(target_os = "linux")]
    {
        OFlags::NOATIME
    }
    #[cfg(not(target_os = "linux"))]
    {
        OFlags::empty()
    }
}

/// Open read-only, without touching the access time where the kernel allows
/// it. Linux grants `O_NOATIME` only for a file the caller owns (or with
/// `CAP_FOWNER`) and refuses any other with `EPERM`, so a reader that is not
/// root opens `/` and root's directories without it. Found on 2026-10-14 when
/// the ARM64 tests first ran as an ordinary user.
fn open_read(dir: &OwnedFd, name: &std::ffi::OsStr, flags: OFlags) -> rustix::io::Result<OwnedFd> {
    match fs::openat(dir, name, flags | no_atime(), Mode::empty()) {
        Err(rustix::io::Errno::PERM) if !no_atime().is_empty() => {
            fs::openat(dir, name, flags, Mode::empty())
        }
        opened => opened,
    }
}

fn owned(stat: &fs::Stat) -> bool {
    stat.st_uid == rustix::process::geteuid().as_raw() || stat.st_uid == 0
}

fn trusted_directory(fd: &OwnedFd, last_private: bool) -> io::Result<()> {
    let stat = fs::fstat(fd)?;
    let writable = stat.st_mode & 0o022 != 0;
    let sticky_root = stat.st_uid == 0 && stat.st_mode & 0o1000 != 0;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || !owned(&stat)
        || (writable && !sticky_root)
        || (last_private && stat.st_mode & 0o077 != 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "untrusted JSON file directory",
        ));
    }
    Ok(())
}

fn parent(path: &Path, private: bool) -> io::Result<OwnedFd> {
    absolute_path(path)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = match fs::open("/", flags | no_atime(), Mode::empty()) {
        Err(rustix::io::Errno::PERM) if !no_atime().is_empty() => {
            fs::open("/", flags, Mode::empty())?
        }
        opened => opened?,
    };
    trusted_directory(&fd, false)?;
    for component in path
        .parent()
        .ok_or_else(|| invalid("missing parent"))?
        .components()
    {
        if let Component::Normal(name) = component {
            fd = open_read(&fd, name, flags)?;
            trusted_directory(&fd, false)?;
        }
    }
    trusted_directory(&fd, private)?;
    Ok(fd)
}

fn regular(stat: &fs::Stat) -> io::Result<()> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || !owned(stat)
        || stat.st_mode & 0o022 != 0
        || stat.st_nlink != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "JSON file must be a trusted, singly linked regular file",
        ));
    }
    Ok(())
}

fn stamp(stat: &Metadata) -> Stamp {
    Stamp {
        device: stat.dev(),
        inode: stat.ino(),
        size: stat.size(),
        modified_s: stat.mtime(),
        modified_ns: stat.mtime_nsec(),
        changed_s: stat.ctime(),
        changed_ns: stat.ctime_nsec(),
    }
}

/// A matching stamp avoids repeatedly parsing the last result between runs.
pub fn read_changed<T: DeserializeOwned>(
    path: &Path,
    limit: usize,
    prior: Option<&Stamp>,
) -> io::Result<Option<(T, Stamp)>> {
    let dir = parent(path, false)?;
    let Some((bytes, stamp)) = read_bytes(&dir, path, limit, prior)? else {
        return Ok(None);
    };
    let value = serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    Ok(Some((value, stamp)))
}

fn read_bytes(
    dir: &OwnedFd,
    path: &Path,
    limit: usize,
    prior: Option<&Stamp>,
) -> io::Result<Option<(Vec<u8>, Stamp)>> {
    let fd = open_read(
        dir,
        path.file_name().unwrap(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
    )?;
    let stat = fs::fstat(&fd)?;
    regular(&stat)?;
    let mut file = File::from(fd);
    let before = stamp(&file.metadata()?);
    if before.size > limit as u64 {
        return Err(invalid("JSON file exceeds its size bound"));
    }
    if prior == Some(&before) {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(before.size as usize);
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit || stamp(&file.metadata()?) != before {
        return Err(invalid(
            "JSON file changed during read or exceeded its bound",
        ));
    }
    Ok(Some((bytes, before)))
}

pub fn read<T: DeserializeOwned>(path: &Path, limit: usize) -> io::Result<T> {
    read_changed(path, limit, None)?
        .map(|(value, _)| value)
        .ok_or_else(|| io::Error::other("missing JSON result"))
}

/// A newer report is retained whole, never used as current cleanup evidence.
#[derive(Debug)]
pub enum ReportRead {
    Current(RunReport),
    SetAside,
}

fn decode_report(bytes: &[u8]) -> io::Result<Result<RunReport, u64>> {
    match serde_json::from_slice::<RunReport>(bytes) {
        Ok(report) => Ok(Ok(report)),
        Err(_) => match crate::newer_report_schema(bytes)? {
            Some(schema) => Ok(Err(schema)),
            None => Err(invalid("unrecognized cleanup report")),
        },
    }
}

/// Read a changed report with the ordinary trust and size checks. A newer
/// schema is moved aside under the same lock as writers; an observed
/// replacement, collision or failure leaves it untrusted and is an error.
pub fn read_report_changed(
    path: &Path,
    prior: Option<&Stamp>,
    at_unix_s: u64,
) -> io::Result<Option<(ReportRead, Stamp)>> {
    let dir = parent(path, false)?;
    let Some((bytes, stamp)) = read_bytes(&dir, path, REPORT_BYTES, prior)? else {
        return Ok(None);
    };
    match decode_report(&bytes)? {
        Ok(report) => Ok(Some((ReportRead::Current(report), stamp))),
        Err(schema) => {
            trusted_directory(&dir, true)?;
            fs::flock(&dir, fs::FlockOperation::NonBlockingLockExclusive)?;
            set_aside_report(&dir, path, &stamp, schema, at_unix_s)?;
            Ok(Some((ReportRead::SetAside, stamp)))
        }
    }
}

fn set_aside_report(
    dir: &OwnedFd,
    path: &Path,
    expected: &Stamp,
    schema: u64,
    at_unix_s: u64,
) -> io::Result<()> {
    // A writer may have replaced the result between its read and this lock.
    // The private directory and cooperating-writer lock protect the rename;
    // a changed source is never moved under an earlier result's identity.
    if read_bytes(dir, path, REPORT_BYTES, Some(expected))?.is_some() {
        return Err(invalid(
            "cleanup result changed before it could be set aside",
        ));
    }
    let name = path.file_name().unwrap();
    let kept = format!("{}.newer-{schema}-{at_unix_s}", name.to_str().unwrap());
    fs::renameat_with(dir, name, dir, kept.as_str(), RenameFlags::NOREPLACE)?;
    fs::fsync(dir)?;
    eprintln!(
        "lamp-records: cleanup result is newer than this build (schema {schema}); kept as {kept}; awaiting a new cleanup result"
    );
    Ok(())
}

pub fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    absolute_path(path)?;
    Ok(path.with_file_name(format!(
        ".{}.lamp-records-tmp",
        path.file_name().unwrap().to_str().unwrap()
    )))
}

/// Path equality alone misses case-folded names. Compare validated descriptors
/// before permitting an output or its reserved temporary to replace an input.
pub fn same_file(first: &Path, second: &Path) -> io::Result<bool> {
    fn identity(path: &Path) -> io::Result<(u64, u64)> {
        let dir = parent(path, false)?;
        let file = File::from(open_read(
            &dir,
            path.file_name().unwrap(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        )?);
        regular(&fs::fstat(&file)?)?;
        let metadata = file.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }
    let first = identity(first)?;
    match identity(second) {
        Ok(second) => Ok(first == second),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The report's parent must already exist and be private. No record root is
/// created as a side effect. Only this process's newly created temp is removed.
pub fn write<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > REPORT_BYTES {
        return Err(invalid("report exceeds its size bound"));
    }
    let dir = parent(path, true)?;
    fs::flock(&dir, fs::FlockOperation::NonBlockingLockExclusive)?;
    write_locked(&dir, path, &bytes)
}

/// Recheck the prior report while holding the writer lock, including when a
/// newer build replaced it after the cleanup's preflight. Unknown same-schema
/// values and malformed files are refused without overwriting them.
pub fn write_report(path: &Path, report: &RunReport) -> io::Result<()> {
    report.validate()?;
    let bytes = serde_json::to_vec(report).map_err(io::Error::other)?;
    if bytes.len() > REPORT_BYTES {
        return Err(invalid("report exceeds its size bound"));
    }
    let dir = parent(path, true)?;
    fs::flock(&dir, fs::FlockOperation::NonBlockingLockExclusive)?;
    match read_bytes(&dir, path, REPORT_BYTES, None) {
        Ok(Some((prior, stamp))) => {
            if let Err(schema) = decode_report(&prior)? {
                set_aside_report(&dir, path, &stamp, schema, report.at_unix_s)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(None) => unreachable!("an uncached read always returns the file"),
    }
    write_locked(&dir, path, &bytes)
}

fn write_locked(dir: &OwnedFd, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().unwrap();
    match fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => regular(&stat)?,
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
    // A single reserved temp per output, under the writer lock, bounds crash
    // residue. Never remove a linked/special/untrusted replacement there.
    let temporary = temporary_path(path)?;
    let temp = temporary.file_name().unwrap().to_str().unwrap();
    match fs::statat(dir, temp, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            regular(&stat)?;
            if stat.st_mode & 0o7777 != 0o600 {
                return Err(invalid("temporary report file is not private"));
            }
            fs::unlinkat(dir, temp, AtFlags::empty())?;
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
    let fd = fs::openat(
        dir,
        temp,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    let mut file = File::from(fd);
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::renameat(dir, temp, dir, name)?;
        fs::fsync(dir)?;
        Ok(())
    })();
    // After successful rename this name is absent. It was exclusively ours;
    // trusted private-parent ownership is the namespace contract here too.
    if result.is_err() {
        let _ = fs::unlinkat(dir, temp, AtFlags::empty());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs as stdfs,
        os::unix::fs::{PermissionsExt, symlink},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn scratch() -> std::path::PathBuf {
        let dir = stdfs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "lamp-records-json-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        stdfs::create_dir(&dir).unwrap();
        stdfs::set_permissions(&dir, stdfs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn report() -> RunReport {
        RunReport {
            schema_version: crate::REPORT_SCHEMA_VERSION,
            config_fingerprint: "0123456789abcdef".repeat(4),
            at_unix_s: 17,
            mode: "apply".into(),
            enabled: false,
            roots: vec![],
            error: None,
        }
    }

    fn newer() -> Vec<u8> {
        let mut value = serde_json::to_value(report()).unwrap();
        value["schema_version"] = 2.into();
        value["mode"] = "later_mode".into();
        value["roots"] = serde_json::json!([{"later_kind": "new_enum_value"}]);
        serde_json::to_vec_pretty(&value).unwrap()
    }

    #[test]
    fn current_report_ignores_added_fields_and_keeps_the_stamp_cache() {
        let dir = scratch();
        let path = dir.join("last.json");
        let mut later = serde_json::to_value(report()).unwrap();
        later["later"] = serde_json::json!({"kind": "future_detail"});
        write(&path, &later).unwrap();
        let (value, stamp) = read_report_changed(&path, None, 17).unwrap().unwrap();
        let ReportRead::Current(value) = value else {
            panic!("added fields must not set aside the current schema")
        };
        assert_eq!(
            serde_json::to_value(&value).unwrap(),
            serde_json::to_value(report()).unwrap()
        );
        assert!(
            read_report_changed(&path, Some(&stamp), 18)
                .unwrap()
                .is_none()
        );
        write_report(&path, &value).unwrap();
        assert_eq!(
            stdfs::read(&path).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
        assert_eq!(stdfs::read_dir(&dir).unwrap().count(), 1);
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_report_is_kept_whole_once_without_creating_cleanup_evidence() {
        let dir = scratch();
        let path = dir.join("last.json");
        let bytes = newer();
        stdfs::write(&path, &bytes).unwrap();
        let (value, _) = read_report_changed(&path, None, 17).unwrap().unwrap();
        assert!(matches!(value, ReportRead::SetAside));
        assert!(!path.exists());
        let kept = dir.join("last.json.newer-2-17");
        assert_eq!(stdfs::read(&kept).unwrap(), bytes);
        assert_eq!(
            read_report_changed(&path, None, 18).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(stdfs::read_dir(&dir).unwrap().count(), 1);
        write_report(&path, &report()).unwrap();
        assert!(matches!(
            read_report_changed(&path, None, 18).unwrap().unwrap().0,
            ReportRead::Current(_)
        ));
        assert_eq!(stdfs::read(kept).unwrap(), bytes);
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn final_writer_rechecks_and_preserves_a_newer_report_before_replacing_it() {
        let dir = scratch();
        let path = dir.join("last.json");
        write_report(&path, &report()).unwrap();
        // The cleanup's preflight read an ordinary receipt. A newer writer
        // installs its report before this cleanup reaches persistence.
        read_report_changed(&path, None, 16).unwrap().unwrap();
        let bytes = newer();
        stdfs::write(&path, &bytes).unwrap();
        write_report(&path, &report()).unwrap();
        assert_eq!(
            stdfs::read(dir.join("last.json.newer-2-17")).unwrap(),
            bytes
        );
        assert_eq!(
            stdfs::read(&path).unwrap(),
            serde_json::to_vec(&report()).unwrap()
        );
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_collision_or_source_replacement_never_moves_or_overwrites_either_file() {
        let dir = scratch();
        let path = dir.join("last.json");
        let kept = dir.join("last.json.newer-2-17");
        let bytes = newer();
        stdfs::write(&path, &bytes).unwrap();
        stdfs::write(&kept, b"earlier archive").unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        assert!(write_report(&path, &report()).is_err());
        assert_eq!(stdfs::read(&path).unwrap(), bytes);
        assert_eq!(stdfs::read(&kept).unwrap(), b"earlier archive");
        let parent = parent(&path, true).unwrap();
        let (_, prior) = read_bytes(&parent, &path, REPORT_BYTES, None)
            .unwrap()
            .unwrap();
        write(&path, &report()).unwrap();
        fs::flock(&parent, fs::FlockOperation::NonBlockingLockExclusive).unwrap();
        assert!(set_aside_report(&parent, &path, &prior, 2, 18).is_err());
        assert_eq!(
            stdfs::read(&path).unwrap(),
            serde_json::to_vec(&report()).unwrap()
        );
        assert!(!dir.join("last.json.newer-2-18").exists());
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unrecognized_or_unknown_supported_values_are_preserved_on_read_and_write() {
        let dir = scratch();
        let path = dir.join("last.json");
        let mut unknown_mode = serde_json::to_value(report()).unwrap();
        unknown_mode["mode"] = "later_mode".into();
        unknown_mode["later"] = true.into();
        for bytes in [
            b"{malformed".to_vec(),
            br#"{"schema_version":2}"#.to_vec(),
            serde_json::to_vec(&unknown_mode).unwrap(),
        ] {
            stdfs::write(&path, &bytes).unwrap();
            assert!(read_report_changed(&path, None, 17).is_err());
            assert!(write_report(&path, &report()).is_err());
            assert_eq!(stdfs::read(&path).unwrap(), bytes);
            assert_eq!(stdfs::read_dir(&dir).unwrap().count(), 1);
        }
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_report_still_requires_trusted_private_storage_and_the_file_byte_bound() {
        let dir = scratch();
        let target = dir.join("target.json");
        let path = dir.join("last.json");
        let bytes = newer();
        stdfs::write(&target, &bytes).unwrap();
        symlink(&target, &path).unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        assert!(write_report(&path, &report()).is_err());
        stdfs::remove_file(&path).unwrap();
        stdfs::hard_link(&target, &path).unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        assert!(write_report(&path, &report()).is_err());
        assert_eq!(stdfs::read(&target).unwrap(), bytes);
        stdfs::remove_file(&target).unwrap();
        stdfs::set_permissions(&dir, stdfs::Permissions::from_mode(0o755)).unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        assert_eq!(stdfs::read(&path).unwrap(), bytes);
        stdfs::set_permissions(&dir, stdfs::Permissions::from_mode(0o700)).unwrap();
        stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o600)).unwrap();
        let mut oversized = bytes;
        oversized.resize(REPORT_BYTES + 1, b' ');
        stdfs::write(&path, &oversized).unwrap();
        assert!(read_report_changed(&path, None, 17).is_err());
        assert!(write_report(&path, &report()).is_err());
        assert_eq!(stdfs::read(&path).unwrap(), oversized);
        assert_eq!(stdfs::read_dir(&dir).unwrap().count(), 1);
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_result_replaces_only_known_file_and_cache_detects_change() {
        let dir = scratch();
        let path = dir.join("last.json");
        write(&path, &vec![1_u64, 2]).unwrap();
        let (value, stamp) = read_changed::<Vec<u64>>(&path, 100, None).unwrap().unwrap();
        assert_eq!(value, vec![1, 2]);
        assert!(
            read_changed::<Vec<u64>>(&path, 100, Some(&stamp))
                .unwrap()
                .is_none()
        );
        write(&path, &vec![3_u64]).unwrap();
        assert_eq!(read::<Vec<u64>>(&path, 100).unwrap(), vec![3]);
        assert!(
            read_changed::<Vec<u64>>(&path, 100, Some(&stamp))
                .unwrap()
                .is_some()
        );
        assert_eq!(stdfs::read_dir(&dir).unwrap().count(), 1);
        assert!(read::<Vec<u64>>(&path, 1).is_err());
        stdfs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn result_symlink_and_hardlink_are_refused_without_touching_target() {
        let dir = scratch();
        let target = dir.join("outside.json");
        stdfs::write(&target, b"[42]").unwrap();
        let link = dir.join("last.json");
        symlink(&target, &link).unwrap();
        assert!(write(&link, &vec![0]).is_err());
        assert!(read::<Vec<u64>>(&link, 100).is_err());
        stdfs::remove_file(&link).unwrap();
        stdfs::hard_link(&target, &link).unwrap();
        assert!(write(&link, &vec![0]).is_err());
        assert!(read::<Vec<u64>>(&link, 100).is_err());
        assert_eq!(stdfs::read(&target).unwrap(), b"[42]");
        stdfs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_private_reserved_temporary_is_recovered_after_an_interrupted_write() {
        let dir = scratch();
        let path = dir.join("last.json");
        let temp = dir.join(".last.json.lamp-records-tmp");
        stdfs::write(&temp, b"partial JSON from an interrupted write").unwrap();
        stdfs::set_permissions(&temp, stdfs::Permissions::from_mode(0o600)).unwrap();
        write(&path, &vec![7_u64]).unwrap();
        assert_eq!(read::<Vec<u64>>(&path, 100).unwrap(), vec![7]);
        assert!(!temp.exists());
        symlink(&path, &temp).unwrap();
        assert!(write(&path, &vec![8_u64]).is_err());
        assert_eq!(read::<Vec<u64>>(&path, 100).unwrap(), vec![7]);
        stdfs::remove_dir_all(dir).unwrap();
    }
}
