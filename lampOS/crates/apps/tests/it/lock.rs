use lamp_apps::lock::{AppsLock, BUSY};
use std::time::Duration;

#[test]
fn a_second_holder_waits_then_is_told_busy_and_the_first_release_frees_it() {
    let dir = tempfile::tempdir().unwrap();
    let first = AppsLock::take(dir.path(), Duration::from_secs(2)).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        AppsLock::take(dir.path(), Duration::from_millis(200))
            .err()
            .as_deref(),
        Some(BUSY)
    );
    assert!(started.elapsed() >= Duration::from_millis(150));
    drop(first);
    assert!(AppsLock::take(dir.path(), Duration::from_secs(2)).is_ok());
}

#[test]
fn the_lock_files_are_root_only_and_the_folder_stays_readable() {
    use lamp_apps::lock::{AppsLock, Running};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let _a = AppsLock::take(&state, Duration::from_millis(300)).unwrap();
    let _r = Running::shared(&state, Duration::from_millis(300)).unwrap();
    let mode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(state.join(".lock")), 0o600);
    assert_eq!(mode(state.join(".running")), 0o600);
    // lamp-cloud reads the marks in it.
    assert_eq!(mode(state.clone()), 0o755);
}

#[test]
fn a_lock_file_that_is_a_link_is_not_opened() {
    use lamp_apps::lock::AppsLock;
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), state.join(".lock")).unwrap();
    assert!(AppsLock::take(&state, Duration::from_millis(100)).is_err());
    assert!(!dir.path().join("elsewhere").exists());
}
