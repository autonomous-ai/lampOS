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
