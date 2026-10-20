use lamp_surround::{
    atomic,
    config::{ProviderDocument, Secret},
    configuration::*,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}
fn document() -> ProviderDocument {
    ProviderDocument::parse(br#"{"schema_version":1,"provider":{"endpoint":"wss://provider.example/live","model":"gemini-3.8-live","voice":"Kore","credential_file":"/unused/key"}}"#).unwrap()
}
fn stage(store: &Configurations, id: u8) -> Generation {
    store
        .stage(
            &format!("{id:032x}"),
            document(),
            Secret::new(b"fixture-key".to_vec()).unwrap(),
            Some("Answer briefly."),
        )
        .unwrap()
}

#[test]
fn stage_publishes_a_whole_private_generation_without_activating_it() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let first = stage(&store, 1);
    assert_eq!(store.candidate().unwrap(), Some(first.clone()));
    assert_eq!(store.selection().unwrap(), Selection::default());
    let path = store.provider_path(&first).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(payload.get("schema_version").is_none());
    assert_eq!(
        payload["credential_file"],
        path.with_file_name("provider.key").to_str().unwrap()
    );
    for name in [
        "provider.key",
        "document.json",
        "provider.json",
        "instructions.txt",
        "manifest.json",
    ] {
        assert_eq!(
            fs::metadata(path.with_file_name(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("fixture-key")
    );
}

#[test]
fn compare_select_preserves_previous_and_needs_explicit_enablement() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let b = stage(&store, 2);
    assert!(store.select_quiescent(false, &None, &a).is_err());
    assert_eq!(
        store.select_quiescent(true, &None, &a).unwrap().current,
        Some(a.clone())
    );
    assert!(store.select_quiescent(true, &None, &b).is_err());
    let selected = store.select_quiescent(true, &Some(a.clone()), &b).unwrap();
    assert_eq!(selected.previous, Some(a));
    assert_eq!(
        store.select_quiescent(true, &Some(b.clone()), &b).unwrap(),
        selected
    );
    assert_eq!(
        Configurations::open(root.path())
            .unwrap()
            .selection()
            .unwrap(),
        selected
    );
}

#[test]
fn a_generation_is_never_overwritten_and_partial_staging_never_replaces_candidate() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let original = fs::read(store.provider_path(&a).unwrap()).unwrap();
    assert!(
        store
            .stage(
                &a.id,
                document(),
                Secret::new(b"different-key".to_vec()).unwrap(),
                None
            )
            .is_err()
    );
    let partial = root.path().join("generations").join(format!("{:032x}", 2));
    atomic::ensure_private_dir(&partial).unwrap();
    atomic::write(&partial.join("provider.key"), b"partial", 4096).unwrap();
    let forged = Generation {
        id: format!("{:032x}", 2),
        document_sha256: "a".repeat(64),
    };
    assert!(store.verify(&forged).is_err());
    assert!(store.select_quiescent(true, &None, &forged).is_err());
    assert_eq!(store.candidate().unwrap(), Some(a.clone()));
    assert_eq!(
        fs::read(store.provider_path(&a).unwrap()).unwrap(),
        original
    );
}

#[test]
fn secret_or_payload_tampering_and_link_substitution_fail_before_selection() {
    for file in [
        "provider.key",
        "provider.json",
        "document.json",
        "instructions.txt",
        "manifest.json",
    ] {
        let root = dir();
        let store = Configurations::open(root.path()).unwrap();
        let a = stage(&store, 1);
        let file = store.provider_path(&a).unwrap().with_file_name(file);
        atomic::write(&file, b"changed", 16384).unwrap();
        assert!(store.select_quiescent(true, &None, &a).is_err());
        assert_eq!(store.selection().unwrap(), Selection::default());
    }
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let secret = store
        .provider_path(&a)
        .unwrap()
        .with_file_name("provider.key");
    let outside = root.path().join("outside.key");
    fs::rename(&secret, &outside).unwrap();
    symlink(&outside, &secret).unwrap();
    assert!(store.verify(&a).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"fixture-key");
}

#[test]
fn repeated_staging_reclaims_superseded_data_and_keeps_current_previous_and_candidate() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    store.select_quiescent(true, &None, &a).unwrap();
    let b = stage(&store, 2);
    store.select_quiescent(true, &Some(a.clone()), &b).unwrap();
    for i in 3..(MAX_GENERATIONS as u8 + 8) {
        stage(&store, i);
    }
    store.provider_path(&a).unwrap();
    store.provider_path(&b).unwrap();
    assert_eq!(store.selection().unwrap().current, Some(b));
    assert_eq!(
        fs::read_dir(root.path().join("generations"))
            .unwrap()
            .count(),
        4
    );
    store.collect_superseded().unwrap();
    assert_eq!(
        fs::read_dir(root.path().join("generations"))
            .unwrap()
            .count(),
        3
    );
}

#[test]
fn collection_resumes_after_partial_delete_but_never_deletes_links_or_unknown_files() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let b = stage(&store, 2);
    let orphan = store
        .provider_path(&a)
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    atomic::remove(&orphan.join("provider.key")).unwrap();
    assert_eq!(store.collect_superseded().unwrap(), vec![a.id]);
    store.provider_path(&b).unwrap();
    for case in 0..3 {
        let name = format!("{:032x}", 10 + case);
        let orphan = root.path().join("generations").join(&name);
        atomic::ensure_private_dir(&orphan).unwrap();
        let outside = root.path().join(format!("outside{case}.key"));
        atomic::write(&outside, b"keep", 16).unwrap();
        match case {
            0 => symlink(&outside, orphan.join("provider.key")).unwrap(),
            1 => fs::hard_link(&outside, orphan.join("provider.key")).unwrap(),
            _ => atomic::write(&orphan.join("unknown.json"), b"keep", 16).unwrap(),
        }
        assert!(store.collect_superseded().is_err());
        assert_eq!(fs::read(outside).unwrap(), b"keep");
        assert!(orphan.exists());
        fs::remove_dir_all(orphan).unwrap();
    }
}

#[test]
fn invalid_input_does_not_allocate_a_directory_and_empty_optional_instructions_work() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    for blank in ["", "   \n", "\u{2003}\u{a0}"] {
        assert!(
            store
                .stage(
                    &"f".repeat(32),
                    document(),
                    Secret::new(b"fixture-key".to_vec()).unwrap(),
                    Some(blank)
                )
                .is_err()
        );
    }
    for bad_id in ["../../outside", "", "0", &"A".repeat(32)] {
        assert!(
            store
                .stage(
                    bad_id,
                    document(),
                    Secret::new(b"fixture-key".to_vec()).unwrap(),
                    None
                )
                .is_err()
        );
    }
    assert!(
        store
            .stage(
                &"0".repeat(32),
                document(),
                Secret::new(b"fixture-key".to_vec()).unwrap(),
                Some(&"a".repeat(MAX_INSTRUCTIONS + 1))
            )
            .is_err()
    );
    assert_eq!(
        fs::read_dir(root.path().join("generations"))
            .unwrap()
            .count(),
        0
    );
    let a = store
        .stage(
            &"0".repeat(32),
            document(),
            Secret::new(b"fixture-key".to_vec()).unwrap(),
            None,
        )
        .unwrap();
    store.verify(&a).unwrap();
    assert!(
        !store
            .provider_path(&a)
            .unwrap()
            .with_file_name("instructions.txt")
            .exists()
    );
}

#[test]
fn publication_failure_leaves_old_selection_and_retry_cannot_mutate_the_orphan() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    store.select_quiescent(true, &None, &a).unwrap();
    fs::create_dir(root.path().join(".candidate.json.pending")).unwrap();
    assert!(
        store
            .stage(
                &format!("{:032x}", 2),
                document(),
                Secret::new(b"fixture-key".to_vec()).unwrap(),
                None
            )
            .is_err()
    );
    assert_eq!(store.selection().unwrap().current, Some(a.clone()));
    assert_eq!(store.candidate().unwrap(), Some(a));
    fs::remove_dir(root.path().join(".candidate.json.pending")).unwrap();
    assert!(
        store
            .stage(
                &format!("{:032x}", 2),
                document(),
                Secret::new(b"another-key".to_vec()).unwrap(),
                None
            )
            .is_err()
    );
}

#[test]
fn a_damaged_current_generation_does_not_block_healthy_rollback() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let b = stage(&store, 2);
    let b_key = store
        .provider_path(&b)
        .unwrap()
        .with_file_name("provider.key");
    store.select_quiescent(true, &None, &a).unwrap();
    store.select_quiescent(true, &Some(a.clone()), &b).unwrap();
    atomic::write(&b_key, b"broken", 4096).unwrap();
    assert!(store.verify(&b).is_err());
    assert_eq!(
        store.select_quiescent(true, &Some(b), &a).unwrap().current,
        Some(a.clone())
    );
    store.provider_path(&a).unwrap();
}

#[test]
fn a_damaged_previous_generation_does_not_hide_healthy_current() {
    let root = dir();
    let store = Configurations::open(root.path()).unwrap();
    let a = stage(&store, 1);
    let b = stage(&store, 2);
    let a_key = store
        .provider_path(&a)
        .unwrap()
        .with_file_name("provider.key");
    store.select_quiescent(true, &None, &a).unwrap();
    store.select_quiescent(true, &Some(a), &b).unwrap();
    atomic::remove(&a_key).unwrap();
    assert_eq!(store.selection().unwrap().current, Some(b.clone()));
    store.provider_path(&b).unwrap();
}
