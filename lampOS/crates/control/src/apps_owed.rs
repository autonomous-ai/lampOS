//! What the apps' helper is still to be told of a pairing change (Heron's P1
//! on #8358): a revoked computer's pushed apps stopped (`off_pushed_by`), or
//! a narrowed pairing's held to its grants (`grants_pushed_by`). Each change
//! is kept in `--state-dir`'s [`OWED`] (0600) before it is sent, and let go
//! only once the helper answered it, so a restart of lamp-control in between,
//! or a helper that was down, can't skip it: at its start lamp-control tells
//! the helper every change still kept, in order. Asking again is harmless:
//! each change sets a computer's apps' state.
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;

/// The changes still owed, in `--state-dir`.
pub const OWED: &str = "apps-helper-owed.json";
/// The most changes kept: past it the oldest goes (a re-tell of a later one
/// for the same computer stands for it in practice).
pub const MOST: usize = 256;

/// One writer at a time (the door's revoke, the telling thread).
static FILE: Mutex<()> = Mutex::new(());

/// The changes owed, oldest first, from `path`; only the two verbs.
pub fn owed(path: &Path) -> Vec<Value> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    read(path)
}

fn read(path: &Path) -> Vec<Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .map(|changes| changes.into_iter().filter(is_change).collect())
        .unwrap_or_default()
}

fn is_change(change: &Value) -> bool {
    change.get("off_pushed_by").is_some_and(Value::is_string)
        || change
            .get("grants_pushed_by")
            .is_some_and(|held| held["client"].is_string() && held["grants"].is_array())
}

fn write(path: &Path, changes: &[Value]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| OWED.to_owned());
    let temporary = path.with_file_name(format!(".{name}.new"));
    let _ = std::fs::remove_file(&temporary);
    let kept = |error: std::io::Error| format!("the apps' change could not be kept: {error}");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(kept)?;
    file.write_all(json!(changes).to_string().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(kept)?;
    std::fs::rename(&temporary, path).map_err(kept)
}

/// `change` kept at the end of what is owed, on disk before it returns.
pub fn keep(path: &Path, change: &Value) -> Result<(), String> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut changes = read(path);
    changes.push(change.clone());
    if changes.len() > MOST {
        changes.drain(..changes.len() - MOST);
    }
    write(path, &changes)
}

/// The first owed change equal to `change` let go: the helper answered it.
pub fn answered(path: &Path, change: &Value) {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut changes = read(path);
    if let Some(at) = changes.iter().position(|kept| kept == change) {
        changes.remove(at);
        if let Err(why) = write(path, &changes) {
            eprintln!("lamp-control: {why}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_kept_in_order_until_answered_and_another_kind_is_never_read() {
        let dir = std::env::temp_dir().join(format!("lc-owed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(OWED);
        assert!(owed(&path).is_empty());
        let off = json!({"off_pushed_by": "c_1"});
        let held = json!({"grants_pushed_by": {"client": "c_2", "grants": ["speak"]}});
        keep(&path, &off).unwrap();
        keep(&path, &held).unwrap();
        keep(&path, &off).unwrap();
        assert_eq!(owed(&path), [off.clone(), held.clone(), off.clone()]);
        answered(&path, &off);
        assert_eq!(owed(&path), [held.clone(), off.clone()]);
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::write(
            &path,
            r#"[{"remove_pushed":{"client":"c_1","name":"a"}},{"off_pushed_by":"c_3"}]"#,
        )
        .unwrap();
        assert_eq!(owed(&path), [json!({"off_pushed_by": "c_3"})]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
