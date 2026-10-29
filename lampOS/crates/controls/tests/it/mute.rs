use lamp_controls::mute::MuteRecord;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/ct-mute-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("mute.json")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn owner_restart_restores_mute_before_any_permission_is_considered() {
    let root = Root::new();
    let mut record = MuteRecord::load(&root.file(), "boot", 1_000).unwrap();
    assert!(!record.muted);
    assert!(record.request_mute(100));
    record.save(&root.file()).unwrap();
    assert_eq!(
        MuteRecord::load(&root.file(), "boot", 1_000).unwrap(),
        record
    );
    assert_eq!(
        fs::metadata(root.file()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn explicit_clear_survives_owner_restart_and_ignores_retained_mute() {
    let root = Root::new();
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(100);
    record.clear_after_explicit_user_request(200).unwrap();
    record.save(&root.file()).unwrap();
    let mut restored = MuteRecord::load(&root.file(), "boot", 300).unwrap();
    assert!(!restored.request_mute(100));
    assert!(!restored.request_mute(200));
    assert!(!restored.muted);
    assert!(restored.request_mute(201));
    assert!(restored.muted);
}

#[test]
fn a_new_machine_boot_preserves_mute_and_rebases_only_the_request_clock() {
    let root = Root::new();
    let mut record = MuteRecord::new("old-boot").unwrap();
    record.request_mute(1_000_000);
    record.save(&root.file()).unwrap();
    let mut restored = MuteRecord::load(&root.file(), "new-boot", 1).unwrap();
    assert!(restored.muted);
    assert_eq!(restored.clock_id, "new-boot");
    assert_eq!(restored.through_us, 0);
    restored.save(&root.file()).unwrap();
    assert_eq!(
        MuteRecord::load(&root.file(), "new-boot", 1).unwrap(),
        restored
    );
    assert!(restored.request_mute(1));
    restored.clear_after_explicit_user_request(2).unwrap();
    restored.save(&root.file()).unwrap();
    let next_boot = MuteRecord::load(&root.file(), "third-boot", 0).unwrap();
    assert!(!next_boot.muted);
    assert_eq!(next_boot.through_us, 0);
    assert_eq!(next_boot.version, 1);
}

#[test]
fn malformed_future_or_oversized_state_is_not_unmuted() {
    let root = Root::new();
    for bytes in [
        b"{broken".to_vec(),
        vec![b'x'; 4097],
        br#"{"version":1,"clock_id":"boot","muted":false,"through_us":999}"#.to_vec(),
    ] {
        fs::write(root.file(), bytes).unwrap();
        fs::set_permissions(root.file(), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(MuteRecord::load(&root.file(), "boot", 100).is_err());
    }
}

#[test]
fn stale_clear_and_unknown_record_versions_are_refused() {
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(100);
    assert!(record.clear_after_explicit_user_request(99).is_err());
    assert!(record.muted);
    record.version = 2;
    assert!(record.check().is_err());
}

#[test]
fn exposed_or_symlink_state_and_nonprivate_directory_are_refused() {
    let root = Root::new();
    let record = MuteRecord::new("boot").unwrap();
    record.save(&root.file()).unwrap();
    fs::set_permissions(root.file(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(MuteRecord::load(&root.file(), "boot", 100).is_err());
    fs::remove_file(root.file()).unwrap();
    let link = root.0.join("other");
    fs::write(&link, "untouched").unwrap();
    std::os::unix::fs::symlink(&link, root.file()).unwrap();
    assert!(record.save(&root.file()).is_err());
    assert_eq!(fs::read_to_string(link).unwrap(), "untouched");
    fs::remove_file(root.file()).unwrap();
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(record.save(&root.file()).is_err());
}

/// A writer's `.pending` is never overwritten by a save. A start that finds
/// one not the record's own reads as muted, sets it aside as evidence and
/// lets saves run again (#4332).
#[test]
fn an_incomplete_other_write_is_not_overwritten() {
    let root = Root::new();
    MuteRecord::new("boot").unwrap().save(&root.file()).unwrap();
    fs::write(root.0.join(".mute.json.pending"), "incomplete").unwrap();
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(100);
    assert!(record.save(&root.file()).is_err());
    assert!(record.muted);
    assert_eq!(
        fs::read_to_string(root.0.join(".mute.json.pending")).unwrap(),
        "incomplete"
    );
    let (loaded, note) = MuteRecord::load_noting(&root.file(), "boot", 100).unwrap();
    assert!(loaded.muted);
    assert!(note.is_some());
    assert_eq!(
        fs::read_to_string(root.0.join(".mute.json.pending.recovered")).unwrap(),
        "incomplete"
    );
    record.save(&root.file()).unwrap();
}

/// An orphan `.replacement`, with no `.pending` and no record, is never
/// overwritten in place: the start reads as muted and keeps it aside.
#[test]
fn orphan_replacement_reads_as_muted_and_is_kept_aside() {
    let root = Root::new();
    let replacement = root.0.join(".mute.json.replacement");
    fs::write(&replacement, "do not overwrite").unwrap();
    let record = MuteRecord::new("boot").unwrap();
    assert!(record.save(&root.file()).is_err());
    assert_eq!(
        fs::read_to_string(&replacement).unwrap(),
        "do not overwrite"
    );
    assert!(!root.file().exists());
    let (loaded, note) = MuteRecord::load_noting(&root.file(), "boot", 100).unwrap();
    assert!(loaded.muted);
    assert!(note.is_some());
    assert!(!replacement.exists());
    assert_eq!(
        fs::read_to_string(root.0.join(".mute.json.replacement.recovered")).unwrap(),
        "do not overwrite"
    );
    loaded.save(&root.file()).unwrap();
}

#[test]
fn completed_replacements_leave_only_a_private_record() {
    let root = Root::new();
    let mut record = MuteRecord::new("boot").unwrap();
    for at in 1..4 {
        record.request_mute(at);
        record.save(&root.file()).unwrap();
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        assert_eq!(MuteRecord::load(&root.file(), "boot", at).unwrap(), record);
        assert_eq!(
            fs::metadata(root.file()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

/// Writes `text` as the mute record, private as `save` makes it.
fn written(root: &Root, text: &str) {
    fs::write(root.file(), text).unwrap();
    fs::set_permissions(root.file(), fs::Permissions::from_mode(0o600)).unwrap();
}

/// #1505, harm 1: a mute record from a newer build, with a field this
/// build does not know, reads as muted, even one that said unmuted, and is
/// said; it never stops the voice starting.
#[test]
fn a_record_with_a_later_field_reads_as_muted_and_is_said() {
    let root = Root::new();
    written(
        &root,
        r#"{"version":1,"clock_id":"boot","muted":false,"through_us":5,"later":true}"#,
    );
    let (record, note) = MuteRecord::load_noting(&root.file(), "boot", 1_000).unwrap();
    assert!(record.muted);
    assert_eq!(
        record.reason, None,
        "the privacy mute, which only a hold clears"
    );
    let note = note.unwrap();
    assert!(
        note.contains("newer build") && note.contains("later"),
        "{note}"
    );
    assert!(MuteRecord::load(&root.file(), "boot", 1_000).unwrap().muted);
}

/// An unknown `reason` value, and a `version` this build does not have,
/// read as muted too, the reason given up for the stronger mute.
#[test]
fn an_unknown_reason_or_version_reads_as_muted_and_is_said() {
    for text in [
        r#"{"version":1,"clock_id":"boot","muted":true,"through_us":5,"reason":"nap"}"#,
        r#"{"version":2,"clock_id":"boot","muted":false,"through_us":5}"#,
    ] {
        let root = Root::new();
        written(&root, text);
        let (record, note) = MuteRecord::load_noting(&root.file(), "boot", 1_000).unwrap();
        assert!(record.muted, "{text}");
        assert_eq!(record.reason, None, "{text}");
        assert!(note.is_some(), "{text}");
    }
}

/// What is no mute record at all stays an error: never a default.
#[test]
fn what_is_not_a_mute_record_is_still_refused() {
    for text in [
        "not json",
        r#"{"clock_id":"boot","muted":false}"#,
        r#"{"version":0,"clock_id":"boot","muted":false,"through_us":5}"#,
        r#"[1,2]"#,
    ] {
        let root = Root::new();
        written(&root, text);
        assert!(
            MuteRecord::load_noting(&root.file(), "boot", 1_000).is_err(),
            "{text}"
        );
    }
    // Today's own record says nothing.
    let root = Root::new();
    written(
        &root,
        r#"{"version":1,"clock_id":"boot","muted":true,"through_us":5,"reason":"sleep"}"#,
    );
    let (record, note) = MuteRecord::load_noting(&root.file(), "boot", 1_000).unwrap();
    assert!(record.asleep());
    assert_eq!(note, None);
}
