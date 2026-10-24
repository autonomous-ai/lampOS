use super::{Result, error};
use rustix::fs::{Dir, Mode, OFlags};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Node {
    mode: u32,
    uid: u32,
    gid: u32,
    dev: u64,
    ino: u64,
    links: u64,
    size: u64,
    blocks: u64,
    mtime: (i64, i64),
    atime: (i64, i64),
    ctime: (i64, i64),
    kind: &'static str,
    sha256: Option<String>,
}
pub type Snapshot = BTreeMap<String, Node>;
fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOATIME
}
pub fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file: File = rustix::fs::open(path, flags(), Mode::empty())
        .map_err(error)?
        .into();
    let meta = file.metadata().map_err(error)?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > limit as u64 {
        return Err("unexpected/oversized regular fixture record".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > limit {
        return Err("record exceeded original byte bound".into());
    }
    Ok(bytes)
}
fn walk(root: &Path, path: &Path, out: &mut Snapshot, depth: usize) -> Result<()> {
    if depth > 4 || out.len() >= 128 {
        return Err("fixture inventory bound exceeded".into());
    }
    let meta = fs::symlink_metadata(path).map_err(error)?;
    let relative = path
        .strip_prefix(root)
        .map_err(error)?
        .to_str()
        .ok_or("non-UTF8 fixture path")?;
    let kind = if meta.is_file() {
        "file"
    } else if meta.is_dir() {
        "directory"
    } else if meta.file_type().is_symlink() {
        "symlink"
    } else {
        "other"
    };
    let hash = if meta.is_file() {
        Some(super::artifacts::digest(path)?)
    } else {
        None
    };
    out.insert(
        relative.into(),
        Node {
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            dev: meta.dev(),
            ino: meta.ino(),
            links: meta.nlink(),
            size: meta.len(),
            blocks: meta.blocks(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            atime: (meta.atime(), meta.atime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            kind,
            sha256: hash,
        },
    );
    if meta.is_dir() {
        let directory: File = rustix::fs::open(path, flags() | OFlags::DIRECTORY, Mode::empty())
            .map_err(error)?
            .into();
        let mut entries = Vec::new();
        for item in Dir::read_from(&directory).map_err(error)? {
            let item = item.map_err(error)?;
            let name = item.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if entries.len() >= 128 {
                return Err("fixture directory entry bound exceeded".into());
            }
            entries.push(std::ffi::OsString::from_vec(name.to_vec()));
        }
        entries.sort();
        for name in entries {
            walk(root, &path.join(name), out, depth + 1)?;
        }
    }
    Ok(())
}
pub fn snapshot(path: &Path) -> Result<Snapshot> {
    let mut out = BTreeMap::new();
    walk(path, path, &mut out, 0)?;
    Ok(out)
}
pub fn unchanged_except_live(before: &Snapshot, after: &Snapshot, live: &str) -> Result<()> {
    let live_prefix = format!("{live}/");
    for (name, node) in before {
        if name == live || name.starts_with(&live_prefix) {
            continue;
        }
        if after.get(name) != Some(node) {
            return Err(format!(
                "dry/refused cleanup changed immutable fixture node {name}"
            ));
        }
    }
    if before.keys().ne(after.keys()) {
        return Err("dry/refused cleanup changed fixture paths".into());
    }
    Ok(())
}
/// Live contents may advance, but retention must not replace any writer-owned object.
pub fn live_objects_preserved(before: &Snapshot, after: &Snapshot, live: &str) -> Result<()> {
    let prefix = format!("{live}/");
    for (name, node) in before
        .iter()
        .filter(|(n, _)| n.as_str() == live || n.starts_with(&prefix))
    {
        let actual = after.get(name).ok_or("live writer object disappeared")?;
        if (
            node.dev, node.ino, node.kind, node.uid, node.gid, node.mode, node.links,
        ) != (
            actual.dev,
            actual.ino,
            actual.kind,
            actual.uid,
            actual.gid,
            actual.mode,
            actual.links,
        ) {
            return Err("live writer object identity or permissions changed".into());
        }
    }
    Ok(())
}
pub fn session(path: &Path, shared: bool, finished: bool) -> Result<(u64, serde_json::Value)> {
    let data = snapshot(path)?;
    let expected = [
        "",
        "metadata",
        "metadata/events-0.jsonl",
        "metadata/events-1.jsonl",
        "metadata/events-2.jsonl",
        "metadata/events-3.jsonl",
        "metadata/health.json",
    ];
    if data.keys().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(
            "actual producer layout is not exactly metadata plus four segment files and health"
                .into(),
        );
    }
    let dev = data[""].dev;
    for (name, node) in &data {
        let directory = name.is_empty() || name == "metadata";
        let mode = if directory {
            if shared { 0o750 } else { 0o700 }
        } else if shared && name != "metadata/health.json" {
            0o640
        } else {
            0o600
        };
        if node.mode & 0o7777 != mode
            || node.uid != 0
            || node.dev != dev
            || (!directory && node.links != 1)
            || (shared && node.gid != 0)
        {
            return Err(format!(
                "wrong actual producer ownership/mode/link/dev at {name}"
            ));
        }
    }
    let health: serde_json::Value =
        serde_json::from_slice(&read(&path.join("metadata/health.json"), 64 * 1024)?)
            .map_err(error)?;
    if health["writer_finished"] != finished || health["health"]["writer_failed"] != false {
        return Err("actual writer did not report expected completion/health".into());
    }
    let bytes = data.values().try_fold(0u64, |total, node| {
        node.blocks
            .checked_mul(512)
            .and_then(|n| total.checked_add(n))
            .ok_or("allocated-byte overflow")
    })?;
    if bytes <= 1 {
        return Err("actual session has no allocated data for size-pressure control".into());
    }
    let nonempty = data
        .iter()
        .filter(|(name, node)| name.starts_with("metadata/events-") && node.size > 0)
        .count();
    Ok((
        bytes,
        serde_json::json!({"allocated_bytes":bytes,"files":data,"nonempty_segments":nonempty,"generation":health["generation"],"rotations":health["health"]["rotations"],"complete_rotation_coverage":false}),
    ))
}
pub fn private_dir(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir(path).map_err(error)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(error)
}
pub fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(error)?;
    file.write_all(bytes).map_err(error)?;
    file.sync_all().map_err(error)
}
pub fn evidence_root() -> Result<PathBuf> {
    let root =
        PathBuf::from(std::env::var_os("LAMP_PAIR_OUTPUT").ok_or("missing LAMP_PAIR_OUTPUT")?);
    if !root.is_absolute() || fs::canonicalize(&root).map_err(error)? != root {
        return Err("evidence root must be absolute/canonical".into());
    }
    let meta = fs::symlink_metadata(&root).map_err(error)?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o7777 != 0o700 {
        return Err("evidence root must be root-owned0700 directory".into());
    }
    Ok(root)
}
