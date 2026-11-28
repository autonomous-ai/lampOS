//! Local, append-only inventories. Hashing does not copy or upload the source.
use crate::{digest, schema::sha};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILES: usize = 100_000;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub version: u32,
    pub retention: String,
    pub privacy: String,
    pub created_unix_s: u64,
    pub readme_sha256: String,
    pub files: Vec<Entry>,
}

pub fn private_dir(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("create NEW private directory: {e}"))
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("create NEW private file: {e}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("write private file: {e}"))
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}

fn walk(root: &Path, dir: &Path, depth: usize, rows: &mut Vec<Entry>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("archive exceeds 64 directory levels".into());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("read archive: {e}"))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let before = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if before.is_symlink() {
            return Err("archive contains a symlink; use the original regular files".into());
        }
        if before.is_dir() {
            walk(root, &path, depth + 1, rows)?;
            continue;
        }
        if !before.is_file() {
            return Err("archive contains a non-regular file".into());
        }
        if rows.len() == MAX_FILES {
            return Err("archive exceeds 100000 files; inventory separate sessions".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_str()
            .ok_or("archive filenames must be UTF-8")?
            .to_owned();
        let mut file = File::open(&path).map_err(|e| e.to_string())?;
        if !same_file(&before, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("archive changed while opening a file; seal the session first".into());
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut bytes = 0_u64;
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes = bytes.checked_add(n as u64).ok_or("file size overflow")?;
            hasher.update(&buffer[..n]);
        }
        let after = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if after.is_symlink()
            || bytes != before.len()
            || !same_file(&before, &after)
            || !same_file(&before, &file.metadata().map_err(|e| e.to_string())?)
        {
            return Err("archive changed during hashing; seal the session first".into());
        }
        rows.push(Entry {
            path: relative,
            bytes,
            sha256: format!("{:x}", hasher.finalize()),
        });
    }
    Ok(())
}

fn scan(root: &Path) -> Result<Vec<Entry>, String> {
    let metadata = fs::symlink_metadata(root).map_err(|e| format!("read archive root: {e}"))?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err("archive root must be a regular directory".into());
    }
    let mut rows = Vec::new();
    walk(root, root, 0, &mut rows)?;
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    if rows.is_empty() {
        return Err("archive is empty".into());
    }
    Ok(rows)
}

/// Returns exact inventory and README hashes for a v2 session. The output must
/// be outside the raw root, avoiding an inventory that includes itself.
pub fn create(root: &Path, out: &Path) -> Result<(String, String), String> {
    let raw = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("output parent must exist: {e}"))?;
    if parent.starts_with(&raw) {
        return Err("inventory output must be outside the raw root".into());
    }
    let files = scan(root)?;
    let readme = format!(
        "# Private sensor evidence inventory\n\nRetention: keep_all. Never delete or overwrite the raw files, journals, scores,\nfailed runs, or this inventory. Real people's recordings and labels stay local;\ndo not publish this README, inventory, raw files, transcripts, or paths.\n\nThis snapshot contains {} regular files. SHA-256 and exact byte lengths are\nin inventory.json. Keep the original session README with consent, capture\nsetup, clocks, models, binary/base/full-patch provenance and data limits in\nthe raw directory. This generated README does not establish consent.\n\nHashing does not copy files or guarantee future retention. The custodian\nretains originals and local backups. Seal a session while hashing. New files\nor corrected labels require a new inventory and output directory; retain all\nolder versions. Verify before scoring and before the morning evidence read.\n",
        files.len()
    );
    let readme_hash = digest(readme.as_bytes());
    let inventory = Inventory {
        version: 1,
        retention: "keep_all".into(),
        privacy: "local_only".into(),
        created_unix_s: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs(),
        readme_sha256: readme_hash.clone(),
        files,
    };
    let bytes = serde_json::to_vec_pretty(&inventory).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(
            "inventory exceeds the 16 MiB input limit; inventory separate sealed sessions".into(),
        );
    }
    private_dir(out)?;
    write_new(&out.join("README.md"), readme.as_bytes())?;
    write_new(&out.join("inventory.json"), &bytes)?;
    Ok((digest(&bytes), readme_hash))
}

pub fn verify(root: &Path, inventory: &Inventory, readme: &[u8]) -> Result<(), String> {
    if inventory.version != 1
        || inventory.retention != "keep_all"
        || inventory.privacy != "local_only"
        || !sha(&inventory.readme_sha256, 64)
        || digest(readme) != inventory.readme_sha256
    {
        return Err("invalid inventory or README hash mismatch".into());
    }
    let mut previous: Option<&str> = None;
    for row in &inventory.files {
        if row.path.is_empty()
            || !sha(&row.sha256, 64)
            || !Path::new(&row.path)
                .components()
                .all(|p| matches!(p, Component::Normal(_)))
            || previous.is_some_and(|p| p >= row.path.as_str())
        {
            return Err("invalid, duplicate or unsorted inventory entry".into());
        }
        previous = Some(&row.path);
    }
    if scan(root)? != inventory.files {
        return Err("archive differs: a file is missing, added, or has changed bytes".into());
    }
    Ok(())
}
