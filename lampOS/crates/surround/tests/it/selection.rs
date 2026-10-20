use crate::support;
use lamp_surround::{atomic, release::*, selection::*};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};
use support::*;

fn state(fixture: &Fixture) -> PathBuf {
    fixture.config.state_dir.parent().unwrap().join("selection")
}
async fn stage(fixture: &Fixture, sequence: u64) -> VerifiedRelease {
    let manifest = fixture.manifest(sequence);
    fixture.publish(&manifest, sequence, &key(1), FUTURE);
    fixture.stage().await.unwrap()
}

#[tokio::test]
async fn first_install_is_optional_and_disabled_selection_refuses_changes() {
    let f = Fixture::new();
    let release = stage(&f, 1).await;
    let selector = Selector::open(f.config.clone(), state(&f), false).unwrap();
    assert_eq!(selector.current().unwrap(), None);
    assert_eq!(selector.current_release().unwrap(), None);
    assert!(
        selector
            .select(None, &release)
            .unwrap_err()
            .0
            .contains("disabled")
    );
    assert_eq!(selector.current().unwrap(), None);
    assert!(selector.collect(&[]).is_err());
}

#[tokio::test]
async fn collection_supports_more_than_sixteen_updates_and_always_keeps_current_and_previous() {
    let f = Fixture::new();
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let mut current = None;
    let mut previous: Option<String> = None;
    for sequence in 1..=18 {
        selector.collect(&[]).unwrap();
        let release = stage(&f, sequence).await;
        let selected = selector.select(current.as_ref(), &release).unwrap();
        assert_eq!(selector.current_release().unwrap(), Some(release.clone()));
        if let Some(previous) = previous.as_ref() {
            assert_eq!(
                recover_verified(&f.config, previous).unwrap().set_sha256(),
                previous
            );
        }
        previous = Some(release.set_sha256().to_owned());
        current = Some(selected);
        assert!(fs::read_dir(&f.config.release_dir).unwrap().count() <= 9);
    }
    let trust: Value =
        serde_json::from_slice(&fs::read(f.config.state_dir.join("trust.json")).unwrap()).unwrap();
    assert_eq!(trust["trust"]["highest_release_sequence"], 18);
    assert!(trust["trust"]["accepted"].as_object().unwrap().len() <= 3);
}

#[tokio::test]
async fn collection_preserves_explicit_candidate_highest_signed_sequence_and_utc_floors() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let candidate = stage(&f, 2).await;
    let obsolete = stage(&f, 3).await;
    let highest = stage(&f, 4).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    selector.select(None, &first).unwrap();
    let before: Value =
        serde_json::from_slice(&fs::read(f.config.state_dir.join("trust.json")).unwrap()).unwrap();
    let report = selector.collect(&[candidate.set_sha256().into()]).unwrap();
    assert_eq!(report.removed_sets, 1);
    assert_eq!(report.removed_directories, 3);
    assert_eq!(report.removed_files, 3);
    for preserved in [&first, &candidate, &highest] {
        assert_eq!(
            recover_verified(&f.config, preserved.set_sha256()).unwrap(),
            *preserved
        );
    }
    assert!(recover_verified(&f.config, obsolete.set_sha256()).is_err());
    let after: Value =
        serde_json::from_slice(&fs::read(f.config.state_dir.join("trust.json")).unwrap()).unwrap();
    for name in [
        "roots",
        "latest_unix_seconds",
        "highest_release_sequence",
        "highest_set_sha256",
        "timestamp",
        "snapshot",
        "targets",
    ] {
        assert_eq!(before["trust"][name], after["trust"][name]);
    }
    let old = f.manifest(3);
    f.publish(&old, 5, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
}

#[tokio::test]
async fn collection_refuses_foreign_files_symlinks_and_unknown_protection_before_pruning() {
    let f = Fixture::new();
    let old = stage(&f, 1).await;
    let current = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    selector.select(None, &current).unwrap();
    assert!(selector.collect(&["0".repeat(64)]).is_err());
    let foreign = old.binaries()[0]
        .path()
        .parent()
        .unwrap()
        .join("foreign.txt");
    atomic::write(&foreign, b"do not delete", 64).unwrap();
    assert!(selector.collect(&[]).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"do not delete");
    assert!(recover_verified(&f.config, old.set_sha256()).is_ok());
    fs::remove_file(&foreign).unwrap();
    let link = f.config.release_dir.join("0".repeat(64));
    symlink(&f.metadata, &link).unwrap();
    assert!(selector.collect(&[]).is_err());
    assert!(f.metadata.join("targets.json").exists());
    assert!(recover_verified(&f.config, old.set_sha256()).is_ok());
}

#[tokio::test]
async fn interrupted_collection_leaves_only_unaccepted_files_and_retry_removes_them() {
    let f = Fixture::new();
    let old = stage(&f, 1).await;
    let current = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    selector.select(None, &current).unwrap();
    let path = old.binaries()[0].path().parent().unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(selector.collect(&[]).is_err());
    assert!(recover_verified(&f.config, old.set_sha256()).is_err());
    assert_eq!(selector.current_release().unwrap(), Some(current));
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    let report = selector.collect(&[]).unwrap();
    assert_eq!(report.removed_sets, 0);
    assert!(report.removed_directories > 0);
    assert_eq!(fs::read_dir(&f.config.release_dir).unwrap().count(), 3);
}

#[tokio::test]
async fn collection_removes_interrupted_staging_but_never_rewrites_current_binary() {
    let f = Fixture::new();
    let current = stage(&f, 1).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    selector.select(None, &current).unwrap();
    let orphan = f.config.release_dir.join("a".repeat(64));
    atomic::ensure_private_dir(&orphan).unwrap();
    atomic::write(&orphan.join("lamp-voice.partial"), b"interrupted", 100).unwrap();
    atomic::write(
        &orphan.join(".lamp-body.partial.pending"),
        b"interrupted",
        100,
    )
    .unwrap();
    let report = selector.collect(&[]).unwrap();
    assert_eq!(report.removed_sets, 0);
    assert_eq!(report.removed_files, 2);
    assert_eq!(report.removed_directories, 1);
    assert_eq!(selector.current_release().unwrap(), Some(current));
}

#[tokio::test]
async fn one_atomic_record_selects_exactly_three_hashes_and_resolves_only_verified_paths() {
    let f = Fixture::new();
    let release = stage(&f, 1).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let selection = selector.select(None, &release).unwrap();
    assert_eq!(selection.generation(), 1);
    assert_eq!(selection.current_set(), release.set_sha256());
    assert_eq!(selection.previous_set(), None);
    assert_eq!(selector.current_release().unwrap(), Some(release.clone()));
    let record: Value =
        serde_json::from_slice(&fs::read(state(&f).join("current.json")).unwrap()).unwrap();
    assert_eq!(
        record["pointer"]["binaries"],
        serde_json::to_value(&release.manifest().binaries).unwrap()
    );
    assert_eq!(
        selector.select(Some(&selection), &release).unwrap(),
        selection
    );
}

#[tokio::test]
async fn generation_compare_rejects_stale_and_aba_changes() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    assert!(selector.select(None, &second).is_err());
    let b = selector.select(Some(&a), &second).unwrap();
    assert_eq!(b.previous_set(), Some(first.set_sha256()));
    assert!(selector.select(Some(&a), &first).is_err());
    let a_again = selector.select(Some(&b), &first).unwrap();
    assert_eq!(a_again.generation(), 3);
    assert!(selector.select(Some(&a), &second).is_err());
    assert_eq!(selector.current().unwrap(), Some(a_again));
}

#[tokio::test]
async fn restart_recovers_the_whole_record_and_preserves_the_previous_set() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    let b = selector.select(Some(&a), &second).unwrap();
    drop(selector);
    let restored = Selector::open(f.config.clone(), state(&f), true).unwrap();
    assert_eq!(restored.current().unwrap(), Some(b.clone()));
    assert_eq!(restored.current_release().unwrap(), Some(second));
    assert_eq!(
        recover_verified(&f.config, b.previous_set().unwrap()).unwrap(),
        first
    );
}

#[tokio::test]
async fn changed_candidate_files_are_rehashed_before_any_pointer_change() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let selection = selector.select(None, &first).unwrap();
    let path = second.binary("lamp-body").unwrap().path();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(path, b"corrupt").unwrap();
    assert!(selector.select(Some(&selection), &second).is_err());
    assert_eq!(selector.current().unwrap(), Some(selection));
    assert_eq!(selector.current_release().unwrap(), Some(first));
}

#[tokio::test]
async fn damaged_current_binary_does_not_prevent_selecting_a_healthy_previous_set() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    let b = selector.select(Some(&a), &second).unwrap();
    fs::remove_file(second.binary("lamp-voice").unwrap().path()).unwrap();
    drop(selector);
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    assert!(selector.current_release().is_err());
    assert_eq!(selector.current().unwrap(), Some(b.clone()));
    let back = selector.select(Some(&b), &first).unwrap();
    assert_eq!(back.current_set(), first.set_sha256());
    assert_eq!(selector.current_release().unwrap(), Some(first));
}

#[tokio::test]
async fn uncommitted_temporary_record_never_becomes_the_current_selection() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    atomic::write(
        &state(&f).join(".current.json.pending"),
        b"incomplete next record",
        8192,
    )
    .unwrap();
    drop(selector);
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    assert_eq!(selector.current().unwrap(), Some(a.clone()));
    let b = selector.select(Some(&a), &second).unwrap();
    assert_eq!(selector.current().unwrap(), Some(b));
    assert!(!state(&f).join(".current.json.pending").exists());
}

#[tokio::test]
async fn disk_error_does_not_acknowledge_or_replace_the_previous_pointer() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    fs::create_dir(state(&f).join(".current.json.pending")).unwrap();
    assert!(selector.select(Some(&a), &second).is_err());
    assert_eq!(selector.current().unwrap(), Some(a.clone()));
    fs::remove_dir(state(&f).join(".current.json.pending")).unwrap();
    assert!(selector.select(Some(&a), &second).is_ok());
}

#[tokio::test]
async fn missing_corrupt_and_symlinked_selection_records_fail_closed() {
    for case in 0..3 {
        let f = Fixture::new();
        let first = stage(&f, 1).await;
        let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
        selector.select(None, &first).unwrap();
        drop(selector);
        let path = state(&f).join("current.json");
        fs::remove_file(&path).unwrap();
        if case == 1 {
            atomic::write(&path, b"{}", 8192).unwrap();
        } else if case == 2 {
            symlink(f.config.state_dir.join("trust.json"), &path).unwrap();
        }
        assert!(Selector::open(f.config.clone(), state(&f), true).is_err());
    }
}

#[tokio::test]
async fn forged_mixed_binary_set_is_rejected_even_with_a_recomputed_record_checksum() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let second = stage(&f, 2).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    selector.select(None, &first).unwrap();
    let path = state(&f).join("current.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["pointer"]["binaries"][1] = serde_json::to_value(&second.manifest().binaries[1]).unwrap();
    // Serialize via the record's actual field order: the checksum is merely a
    // corruption detector; the signed manifest remains the authority.
    let pointer = &value["pointer"];
    let bytes = format!(
        "{{\"schema_version\":{},\"generation\":{},\"current_set\":{},\"previous_set\":{},\"binaries\":{}}}",
        pointer["schema_version"],
        pointer["generation"],
        pointer["current_set"],
        pointer["previous_set"],
        serde_json::to_string(
            &serde_json::from_value::<Vec<BinarySpec>>(pointer["binaries"].clone()).unwrap()
        )
        .unwrap()
    );
    value["sha256"] = json!(sha(bytes.as_bytes()));
    atomic::write(&path, &serde_json::to_vec(&value).unwrap(), 8192).unwrap();
    assert!(
        selector
            .current()
            .unwrap_err()
            .0
            .contains("mixes binary hashes")
    );
}

#[tokio::test]
async fn selection_lock_contention_and_shared_trust_directory_are_rejected() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    assert!(Selector::open(f.config.clone(), f.config.state_dir.clone(), true).is_err());
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let _lock = atomic::DirectoryLock::acquire(&state(&f)).unwrap();
    assert!(selector.select(None, &first).is_err());
}

/// #1505: the selection pointer as a later build writes it, with a field this
/// build doesn't know. The same pointer without it is read; with it, release
/// operations fail today. No fix yet, and no unit starts lamp-surround on a
/// lamp: whoever makes it read one release forward flips this to a plain test
/// (Tests).
#[tokio::test]
#[should_panic(expected = "unknown field")]
async fn a_pointer_from_a_later_build_is_read() {
    let f = Fixture::new();
    let first = stage(&f, 1).await;
    let selector = Selector::open(f.config.clone(), state(&f), true).unwrap();
    let a = selector.select(None, &first).unwrap();
    drop(selector);
    let path = state(&f).join("current.json");
    assert_eq!(
        Selector::open(f.config.clone(), state(&f), true)
            .unwrap()
            .current()
            .unwrap(),
        Some(a.clone())
    );
    let mut later: Value =
        serde_json::from_slice(&atomic::read(&path, 1 << 20).unwrap().unwrap()).unwrap();
    later["a_field_from_a_later_build"] = json!(true);
    atomic::write(&path, &serde_json::to_vec(&later).unwrap(), 1 << 20).unwrap();
    let read = Selector::open(f.config.clone(), state(&f), true).unwrap();
    assert_eq!(read.current().unwrap(), Some(a));
}
