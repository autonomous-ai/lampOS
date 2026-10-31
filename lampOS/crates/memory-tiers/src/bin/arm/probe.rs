//! Isolated benchmark copies live only in this development executable.
use lamp_memory_tiers::{Error, Result, Store};
use rusqlite::{Connection, OpenFlags, backup};
use std::{fs::OpenOptions, io::Write, path::Path};

pub(super) fn fork(root: &Path, owner: &str) -> Result<(Store, tempfile::TempDir)> {
    // The adapter serializes operations and retains the original Store's lock.
    // A separate read-only connection can copy it without exposing a restore
    // constructor in the library or ever replacing the original database.
    let scratch = tempfile::Builder::new().prefix("probe-").tempdir_in(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = scratch.path().join("tiers.sqlite");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&path)?);
    let key = std::fs::read(root.join("tiers.sqlite.forget-key"))?;
    if key.len() != 32 {
        return Err(Error::Invalid("invalid probe fingerprint key"));
    }
    let mut key_file = options.open(scratch.path().join("tiers.sqlite.forget-key"))?;
    key_file.write_all(&key)?;
    key_file.sync_all()?;
    drop(key_file);
    let source =
        Connection::open_with_flags(root.join("tiers.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut destination = Connection::open(&path)?;
    {
        let copy = backup::Backup::new(&source, &mut destination)?;
        if copy.step(-1)? != backup::StepResult::Done {
            return Err(Error::Invalid("probe snapshot incomplete"));
        }
    }
    drop(destination);
    drop(source);
    let store = Store::open(path, owner)?;
    Ok((store, scratch))
}
