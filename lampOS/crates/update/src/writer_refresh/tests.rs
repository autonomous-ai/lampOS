use super::*;
use crate::records::Records;
use serde_json::{Value, json};
use std::os::unix::fs::{FileTypeExt, symlink};
use tempfile::TempDir;

fn binding() -> Binding {
    Binding::new(
        manifest::sha256_hex(b"unique refresh transaction"),
        900_020,
        Artifact::of(b"authenticated envelope"),
        Artifact::of(b"frozen guard"),
        Artifact::of(b"candidate writer"),
        Artifact::of(b"7071 previous writer"),
        Artifact::of(b"complete product state before the first apply"),
    )
}

fn first_apply(binding: &Binding) -> FirstApply {
    FirstApply::new(
        manifest::sha256_hex(b"unique first product transaction"),
        binding.clone(),
        binding.envelope.clone(),
        binding.sequence,
        Artifact::of(b"complete authenticated supported operation plan"),
        Artifact::of(b"complete protected before-images and apply bookkeeping"),
    )
}

fn fixture() -> (TempDir, Store) {
    let folder = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let store = Store::open(folder.path().to_owned()).unwrap();
    (folder, store)
}

fn bytes(store: &Store) -> Vec<u8> {
    fs::read(store.folder.join(JOURNAL)).unwrap()
}

fn document(store: &Store) -> Value {
    serde_json::from_slice(&bytes(store)).unwrap()
}

fn replace(store: &Store, value: &Value) {
    write_whole(
        &store.folder.join(JOURNAL),
        &serde_json::to_vec(value).unwrap(),
        0o600,
    )
    .unwrap();
}

fn probe(session: &mut Session<'_>) {
    session.armed().unwrap();
    session.published().unwrap();
    session.probed().unwrap();
}

/// A retained duplicate models a descriptor held between fork and exec. It
/// does not reproduce an actual spawn or establish a reported failure's cause.
#[test]
fn dropping_the_session_releases_its_lease_despite_a_retained_duplicate() {
    let (_folder, store) = fixture();
    let binding = binding();
    let session = store.prepare(binding.clone()).unwrap();
    let retained = session._lock.try_clone().unwrap();
    let before = bytes(&store);
    assert_eq!(
        store.inspect(&binding).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(session);
    let reopened = Store::open(store.folder.clone()).unwrap();
    assert_eq!(
        reopened.inspect(&binding).unwrap(),
        Admission::RecoveryNeeded
    );
    let recovery = reopened.recover(&binding).unwrap();
    // Closing the old description must not release the independently opened
    // recovery session's lease.
    drop(retained);
    assert_eq!(
        store.inspect(&binding).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(bytes(&store), before);
    drop(recovery);
    assert_eq!(store.inspect(&binding).unwrap(), Admission::RecoveryNeeded);
}

/// Model an inherited descriptor being closed by a process that did not
/// acquire the lease. No real fork or child-side journal operation is run.
#[test]
fn a_nonowner_guard_drop_cannot_unlock_the_acquiring_process_lease() {
    let (_folder, store) = fixture();
    let binding = binding();
    let session = store.prepare(binding.clone()).unwrap();
    let retained = session._lock.try_clone().unwrap();
    let other_pid = std::process::id().wrapping_add(1);
    assert_ne!(other_pid, std::process::id());
    let inherited = OwnedLockGuard {
        file: session._lock.try_clone().unwrap(),
        owner_pid: other_pid,
    };
    drop(inherited);
    assert_eq!(
        store.inspect(&binding).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(session);
    assert_eq!(store.inspect(&binding).unwrap(), Admission::RecoveryNeeded);
    drop(retained);
}

#[test]
fn no_apply_does_not_qualify_but_one_exact_apply_can_enter_without_an_ordinary_round() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    assert_eq!(session.admission(), Admission::AwaitingFirstApply);
    assert!(
        session
            .product_closed(&grant, Artifact::of(b"not a closure"))
            .is_err()
    );
    session.grant(grant.clone()).unwrap();
    assert_eq!(
        session.admission(),
        Admission::OnlyFirstApply(Box::new(grant.clone()))
    );
    let permit = session.claim(&grant).unwrap();
    assert_eq!(session.admission(), Admission::RecoveryNeeded);
    assert!(
        session.claim(&grant).is_err(),
        "a consumed grant was replayed"
    );
    session
        .qualify(
            &permit,
            Artifact::of(b"successful back and first real apply receipt"),
        )
        .unwrap();
    assert_eq!(session.admission(), Admission::ProductRecoveryNeeded);
    session
        .product_closed(&grant, Artifact::of(b"durable product closure receipt"))
        .unwrap();
    assert_eq!(
        session.admission(),
        Admission::Ordinary {
            writer: binding.writer.clone()
        }
    );
    assert!(session.grant(grant.clone()).is_err());
    drop(session);
    assert_eq!(
        store.inspect(&binding).unwrap(),
        Admission::Ordinary {
            writer: binding.writer
        }
    );
    let saved = document(&store);
    assert_eq!(
        saved["grant"]["preimages"],
        serde_json::to_value(grant.preimages).unwrap()
    );
    assert!(saved["qualification"].is_object());
    assert!(saved["product_closed"].is_object());
}

#[test]
fn a_restart_at_every_unqualified_prefix_requires_recovery_and_never_reissues_the_grant() {
    for prefix in 0..=6 {
        let (_folder, store) = fixture();
        let binding = binding();
        let grant = first_apply(&binding);
        let mut session = store.prepare(binding.clone()).unwrap();
        if prefix >= 1 {
            session.armed().unwrap();
        }
        if prefix >= 2 {
            session.published().unwrap();
        }
        if prefix >= 3 {
            session.probed().unwrap();
        }
        if prefix >= 4 {
            session.grant(grant.clone()).unwrap();
        }
        if prefix >= 5 {
            let _permit = session.claim(&grant).unwrap();
        }
        if prefix >= 6 {
            session.restoring().unwrap();
        }
        let before = bytes(&store);
        drop(session);
        let reopened = Store::open(store.folder.clone()).unwrap();
        assert_eq!(
            reopened.inspect(&binding).unwrap(),
            Admission::RecoveryNeeded
        );
        assert!(reopened.prepare(binding.clone()).is_err());
        assert_eq!(bytes(&store), before, "prefix {prefix} was replaced");
        let mut recovery = reopened.recover(&binding).unwrap();
        assert!(recovery.probed().is_err());
        assert!(recovery.grant(grant.clone()).is_err());
        assert!(recovery.claim(&grant).is_err());
        recovery.restoring().unwrap();
        recovery.restored().unwrap();
        drop(recovery);
        assert_eq!(
            reopened.inspect(&binding).unwrap(),
            Admission::Ordinary {
                writer: binding.previous_writer
            }
        );
    }
}

#[test]
fn qualified_writer_and_preimages_survive_a_restart_until_the_product_journal_closes() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    session.grant(grant.clone()).unwrap();
    let permit = session.claim(&grant).unwrap();
    session
        .qualify(&permit, Artifact::of(b"qualified writer receipt"))
        .unwrap();
    drop(session);
    assert_eq!(
        store.inspect(&binding).unwrap(),
        Admission::ProductRecoveryNeeded
    );
    let before = bytes(&store);
    let mut recovery = store.recover(&binding).unwrap();
    assert!(
        recovery.restoring().is_err(),
        "old writer cannot undo the new setting"
    );
    assert_eq!(bytes(&store), before);
    let mut wrong = grant.clone();
    wrong.transaction = manifest::sha256_hex(b"different product transaction");
    assert!(
        recovery
            .product_closed(&wrong, Artifact::of(b"wrong closure"))
            .is_err()
    );
    assert_eq!(bytes(&store), before);
    recovery
        .product_closed(&grant, Artifact::of(b"compatible product rollback closed"))
        .unwrap();
    drop(recovery);
    assert_eq!(
        store.inspect(&binding).unwrap(),
        Admission::Ordinary {
            writer: binding.writer
        }
    );
}

#[test]
fn identity_substitution_never_changes_or_admits_the_saved_transaction() {
    let (_folder, store) = fixture();
    let binding = binding();
    drop(store.prepare(binding.clone()).unwrap());
    let before = bytes(&store);
    let original = serde_json::to_value(&binding).unwrap();
    for field in [
        "envelope",
        "guard",
        "writer",
        "previous_writer",
        "product_before",
    ] {
        for member in ["sha256", "size"] {
            let mut value = original.clone();
            value[field][member] = if member == "sha256" {
                json!(manifest::sha256_hex(b"substitution"))
            } else {
                json!(99)
            };
            let wrong: Binding = serde_json::from_value(value).unwrap();
            assert!(store.inspect(&wrong).is_err(), "accepted {field}.{member}");
            assert_eq!(bytes(&store), before);
        }
    }
    let mut wrong = binding.clone();
    wrong.transaction = manifest::sha256_hex(b"different refresh");
    assert!(store.recover(&wrong).is_err());
    wrong = binding;
    wrong.sequence += 1;
    assert!(store.inspect(&wrong).is_err());
    assert_eq!(bytes(&store), before);
}

#[test]
fn a_stale_or_modified_first_apply_cannot_consume_the_single_grant() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    session.grant(grant.clone()).unwrap();
    let before = bytes(&store);
    for field in ["transaction", "plan", "preimages", "refresh"] {
        let mut wrong = grant.clone();
        match field {
            "transaction" => wrong.transaction = manifest::sha256_hex(b"stale product transaction"),
            "plan" => wrong.plan = Artifact::of(b"different plan"),
            "preimages" => wrong.preimages = Artifact::of(b"different before-images"),
            "refresh" => wrong.refresh.product_before = Artifact::of(b"changed product state"),
            _ => unreachable!(),
        }
        assert!(
            session.claim(&wrong).is_err(),
            "accepted substituted {field}"
        );
        assert_eq!(bytes(&store), before);
    }
    let _permit = session.claim(&grant).unwrap();
    assert!(session.claim(&grant).is_err());
    drop(session);
    assert!(store.recover(&binding).unwrap().claim(&grant).is_err());
}

#[test]
fn unsupported_or_malformed_journals_are_retained_and_block_every_admission() {
    let (_folder, store) = fixture();
    let binding = binding();
    drop(store.prepare(binding.clone()).unwrap());
    let original = document(&store);
    for (field, value) in [
        ("version", json!(2)),
        ("operation", json!("install_arbitrary_unit")),
        ("phase", json!("future_semantic_phase")),
        ("phase", json!("closed")),
        ("grant", json!({"unknown": true})),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        replace(&store, &changed);
        let before = bytes(&store);
        assert!(store.inspect(&binding).is_err(), "accepted {field}");
        assert!(store.recover(&binding).is_err());
        assert!(store.prepare(binding.clone()).is_err());
        assert_eq!(bytes(&store), before);
    }
    for malformed in [
        b"{".as_slice(),
        b"null",
        b"[]",
        b"{\"version\":1,\"version\":2}",
    ] {
        write_whole(&store.folder.join(JOURNAL), malformed, 0o600).unwrap();
        assert!(store.inspect(&binding).is_err());
        assert_eq!(bytes(&store), malformed);
    }
    write_whole(
        &store.folder.join(JOURNAL),
        &vec![b' '; MOST_JOURNAL_BYTES as usize + 1],
        0o600,
    )
    .unwrap();
    let before = bytes(&store);
    assert!(store.inspect(&binding).is_err());
    assert_eq!(bytes(&store), before);
}

#[test]
fn missing_and_nonregular_records_do_not_turn_into_a_fresh_transaction() {
    let (_folder, store) = fixture();
    let binding = binding();
    assert_eq!(
        store.inspect(&binding).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert!(!store.folder.join(JOURNAL).exists());
    let witness = store.folder.join("witness");
    fs::write(&witness, b"unrelated bytes").unwrap();
    symlink(&witness, store.folder.join(JOURNAL)).unwrap();
    assert!(store.inspect(&binding).is_err());
    assert!(store.prepare(binding.clone()).is_err());
    assert_eq!(fs::read(&witness).unwrap(), b"unrelated bytes");
    assert!(
        fs::symlink_metadata(store.folder.join(JOURNAL))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_file(store.folder.join(JOURNAL)).unwrap();
    // rustix's mkfifoat is unavailable on Apple. Both test hosts provide
    // mkfifo; this creates only the private fixture, never a product path.
    assert!(
        std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(store.folder.join(JOURNAL))
            .status()
            .unwrap()
            .success()
    );
    assert!(store.inspect(&binding).is_err());
    assert!(
        fs::symlink_metadata(store.folder.join(JOURNAL))
            .unwrap()
            .file_type()
            .is_fifo()
    );
}

#[test]
fn future_nonsemantic_metadata_is_preserved_without_becoming_admission_authority() {
    let (_folder, store) = fixture();
    let expected = binding();
    let mut value = serde_json::to_value(&expected).unwrap();
    value["future_note"] = json!({"v": 2});
    value["guard"]["future_note"] = json!("guard metadata");
    let with_later: Binding = serde_json::from_value(value).unwrap();
    let mut session = store.prepare(with_later.clone()).unwrap();
    probe(&mut session);
    drop(session);
    let mut saved = document(&store);
    saved["future_note"] = json!([1, 2, 3]);
    replace(&store, &saved);
    assert_eq!(store.inspect(&expected).unwrap(), Admission::RecoveryNeeded);
    let mut recovery = store.recover(&expected).unwrap();
    recovery.restoring().unwrap();
    recovery.restored().unwrap();
    drop(recovery);
    let saved = document(&store);
    assert_eq!(saved["future_note"], json!([1, 2, 3]));
    assert_eq!(saved["binding"]["future_note"], json!({"v": 2}));
    assert_eq!(
        saved["binding"]["guard"]["future_note"],
        json!("guard metadata")
    );
    assert_eq!(
        store.inspect(&expected).unwrap(),
        Admission::Ordinary {
            writer: expected.previous_writer
        }
    );
}

#[test]
fn write_bound_and_invalid_identities_leave_no_journal_or_partial_acceptance() {
    let (_folder, store) = fixture();
    let binding = binding();
    let mut value = serde_json::to_value(&binding).unwrap();
    value["future_note"] = json!("x".repeat(MOST_JOURNAL_BYTES as usize));
    assert!(
        store
            .prepare(serde_json::from_value(value).unwrap())
            .is_err()
    );
    assert!(!store.folder.join(JOURNAL).exists());
    let mut wrong = binding.clone();
    wrong.writer.sha256 = "A".repeat(64);
    assert!(store.prepare(wrong).is_err());
    let mut wrong = binding;
    wrong.guard.size = 0;
    assert!(store.prepare(wrong).is_err());
    assert!(!store.folder.join(JOURNAL).exists());
    assert!(
        !store
            .folder
            .join(format!(".{JOURNAL}.lamp-update"))
            .exists()
    );
}

#[test]
fn diagnostics_hide_unknown_values_and_known_duplicate_or_shadowed_fields_are_refused() {
    let (_folder, store) = fixture();
    let binding = binding();
    drop(store.prepare(binding.clone()).unwrap());
    let mut value = document(&store);
    let private = "future operation containing a person's private words";
    value["operation"] = json!(private);
    replace(&store, &value);
    let before = bytes(&store);
    let error = store.inspect(&binding).unwrap_err().to_string();
    assert!(error.contains("line"));
    assert!(!error.contains(private));
    assert!(!error.contains("private words"));
    assert_eq!(bytes(&store), before);

    value["operation"] = json!("refresh");
    let serialized = serde_json::to_string(&value).unwrap();
    let duplicated = format!("{{\"version\":1,{}", &serialized[1..]);
    write_whole(&store.folder.join(JOURNAL), duplicated.as_bytes(), 0o600).unwrap();
    assert!(store.inspect(&binding).is_err());
    assert_eq!(bytes(&store), duplicated.as_bytes());

    let (_other_folder, other) = fixture();
    let mut shadowed = binding;
    shadowed
        .later
        .insert("writer".to_owned(), json!({"sha256": "shadow"}));
    assert!(other.prepare(shadowed).is_err());
    assert!(!other.folder.join(JOURNAL).exists());
}

#[test]
fn guard_lock_is_exclusive_and_independent_of_the_product_lock() {
    let (_folder, store) = fixture();
    let product = tempfile::tempdir().unwrap();
    let records = Records::new(product.path().to_owned());
    let _product_lock = records.lock().unwrap();
    let binding = binding();
    let session = store.prepare(binding.clone()).unwrap();
    let other = Store::open(store.folder.clone()).unwrap();
    assert_eq!(
        other.inspect(&binding).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(other.prepare(binding.clone()).is_err());
    drop(session);
    assert_eq!(other.inspect(&binding).unwrap(), Admission::RecoveryNeeded);
    let mode = fs::metadata(store.folder.join(JOURNAL))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn failed_persist_after_publication_poisons_the_live_session_until_actual_disk_state_is_recovered()
{
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    session.grant(grant.clone()).unwrap();
    let permit = session.claim(&grant).unwrap();
    store.fail_after_write.set(true);
    assert!(
        session
            .qualify(&permit, Artifact::of(b"successful qualification"))
            .is_err()
    );
    assert_eq!(document(&store)["phase"], json!("qualified"));
    let published = bytes(&store);
    assert_eq!(session.admission(), Admission::RecoveryNeeded);
    assert!(
        session.restoring().is_err(),
        "stale Applying state reverted the qualified writer"
    );
    assert!(
        session
            .qualify(&permit, Artifact::of(b"retry from stale state"))
            .is_err()
    );
    assert!(
        session
            .product_closed(&grant, Artifact::of(b"unproven product closure"))
            .is_err()
    );
    assert_eq!(bytes(&store), published);
    drop(session);
    assert_eq!(
        store.inspect(&binding).unwrap(),
        Admission::ProductRecoveryNeeded
    );
    let mut recovery = store.recover(&binding).unwrap();
    assert!(recovery.restoring().is_err());
    recovery
        .product_closed(&grant, Artifact::of(b"actual product recovery closed"))
        .unwrap();
    drop(recovery);
    assert_eq!(
        store.inspect(&binding).unwrap(),
        Admission::Ordinary {
            writer: binding.writer
        }
    );
}

#[test]
fn first_apply_binds_its_own_later_envelope_and_sequence_without_replacing_the_refresh_identity() {
    let (_folder, store) = fixture();
    let binding = binding();
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    let mut grant = first_apply(&binding);
    grant.sequence += 1;
    grant.envelope = Artifact::of(b"a later independently authenticated build with this writer");
    session.grant(grant.clone()).unwrap();
    let before = bytes(&store);
    let mut wrong = grant.clone();
    wrong.envelope = Artifact::of(b"another later envelope");
    assert!(session.claim(&wrong).is_err());
    wrong = grant.clone();
    wrong.sequence += 1;
    assert!(session.claim(&wrong).is_err());
    assert_eq!(bytes(&store), before);
    let permit = session.claim(&grant).unwrap();
    session
        .qualify(&permit, Artifact::of(b"later first apply qualified"))
        .unwrap();
    let saved = document(&store);
    assert_eq!(saved["binding"]["sequence"], json!(binding.sequence));
    assert_eq!(saved["grant"]["sequence"], json!(binding.sequence + 1));
    assert_ne!(saved["grant"]["envelope"], saved["binding"]["envelope"]);
}

#[test]
fn restoring_an_apply_requires_its_original_grant_and_preimage_identity() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut session = store.prepare(binding.clone()).unwrap();
    probe(&mut session);
    session.grant(grant.clone()).unwrap();
    let _permit = session.claim(&grant).unwrap();
    session.restoring().unwrap();
    drop(session);
    let valid = document(&store);
    assert_eq!(valid["restore_from"], json!("applying"));
    for field in ["grant", "restore_from"] {
        let mut lost = valid.clone();
        lost.as_object_mut().unwrap().remove(field);
        replace(&store, &lost);
        let before = bytes(&store);
        assert!(
            store.inspect(&binding).is_err(),
            "missing {field} was accepted"
        );
        assert!(store.recover(&binding).is_err());
        assert_eq!(bytes(&store), before);
    }
    replace(&store, &valid);
    let mut recovery = store.recover(&binding).unwrap();
    recovery.restoring().unwrap();
    recovery.restored().unwrap();
    assert_eq!(
        document(&store)["grant"]["preimages"],
        serde_json::to_value(grant.preimages).unwrap()
    );
}

#[test]
fn reloaded_inputs_keep_the_exact_origin_and_distinguish_writer_from_apply_restoration() {
    for (prefix, origin) in [
        RecoveryOrigin::Prepared,
        RecoveryOrigin::Armed,
        RecoveryOrigin::Published,
        RecoveryOrigin::Probed,
        RecoveryOrigin::Granted,
        RecoveryOrigin::Applying,
    ]
    .into_iter()
    .enumerate()
    {
        let (_folder, store) = fixture();
        let binding = binding();
        let grant = first_apply(&binding);
        let mut live = store.prepare(binding.clone()).unwrap();
        if prefix >= 1 {
            live.armed().unwrap();
        }
        if prefix >= 2 {
            live.published().unwrap();
        }
        if prefix >= 3 {
            live.probed().unwrap();
        }
        if prefix >= 4 {
            live.grant(grant.clone()).unwrap();
        }
        if prefix >= 5 {
            let _permit = live.claim(&grant).unwrap();
        }
        drop(live);
        let mut recovery = store.recover(&binding).unwrap();
        let before = bytes(&store);
        let expected = if prefix >= 4 {
            RecoveryInputs::RestoreApply {
                binding: &binding,
                first_apply: &grant,
                origin,
                restoration_started: false,
            }
        } else {
            RecoveryInputs::RestoreWriter {
                binding: &binding,
                origin,
                restoration_started: false,
            }
        };
        assert_eq!(recovery.recovery_inputs().unwrap(), expected);
        assert_eq!(bytes(&store), before, "accessor changed prefix {prefix}");
        recovery.restoring().unwrap();
        drop(recovery);

        let mut recovery = store.recover(&binding).unwrap();
        let before = bytes(&store);
        let expected = if prefix >= 4 {
            RecoveryInputs::RestoreApply {
                binding: &binding,
                first_apply: &grant,
                origin,
                restoration_started: true,
            }
        } else {
            RecoveryInputs::RestoreWriter {
                binding: &binding,
                origin,
                restoration_started: true,
            }
        };
        assert_eq!(recovery.recovery_inputs().unwrap(), expected);
        assert_eq!(bytes(&store), before);
        recovery.restored().unwrap();
        assert_eq!(
            recovery.recovery_inputs().unwrap(),
            RecoveryInputs::Ordinary {
                binding: &binding,
                writer: &binding.previous_writer,
                first_apply: (prefix >= 4).then_some(&grant),
                qualification: None,
                product_closed: None,
            }
        );
    }
}

#[test]
fn qualified_and_closed_inputs_keep_the_compatible_writer_and_bound_product_receipts() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let qualification = Artifact::of(b"qualified writer and first apply receipt");
    let closure = Artifact::of(b"product recovery completed with the compatible writer");
    let mut live = store.prepare(binding.clone()).unwrap();
    probe(&mut live);
    live.grant(grant.clone()).unwrap();
    let permit = live.claim(&grant).unwrap();
    live.qualify(&permit, qualification.clone()).unwrap();
    drop(live);

    let mut recovery = store.recover(&binding).unwrap();
    let before = bytes(&store);
    assert_eq!(
        recovery.recovery_inputs().unwrap(),
        RecoveryInputs::RecoverProduct {
            binding: &binding,
            first_apply: &grant,
            qualification: &qualification,
        }
    );
    assert!(recovery.restoring().is_err());
    assert_eq!(bytes(&store), before);
    recovery.product_closed(&grant, closure.clone()).unwrap();
    drop(recovery);

    let recovery = store.recover(&binding).unwrap();
    let before = bytes(&store);
    assert_eq!(
        recovery.recovery_inputs().unwrap(),
        RecoveryInputs::Ordinary {
            binding: &binding,
            writer: &binding.writer,
            first_apply: Some(&grant),
            qualification: Some(&qualification),
            product_closed: Some(&closure),
        }
    );
    assert_eq!(bytes(&store), before);
}

#[test]
fn live_or_poisoned_sessions_cannot_expose_stale_recovery_inputs() {
    let (_folder, store) = fixture();
    let binding = binding();
    let grant = first_apply(&binding);
    let mut live = store.prepare(binding.clone()).unwrap();
    assert!(live.recovery_inputs().is_err());
    probe(&mut live);
    live.grant(grant.clone()).unwrap();
    let _permit = live.claim(&grant).unwrap();
    assert!(live.recovery_inputs().is_err());
    drop(live);

    let mut recovery = store.recover(&binding).unwrap();
    assert!(matches!(
        recovery.recovery_inputs().unwrap(),
        RecoveryInputs::RestoreApply { .. }
    ));
    store.fail_after_write.set(true);
    assert!(recovery.restoring().is_err());
    assert_eq!(document(&store)["phase"], json!("restoring"));
    let before = bytes(&store);
    assert!(recovery.recovery_inputs().is_err());
    assert_eq!(bytes(&store), before);
    drop(recovery);

    let recovery = store.recover(&binding).unwrap();
    assert_eq!(
        recovery.recovery_inputs().unwrap(),
        RecoveryInputs::RestoreApply {
            binding: &binding,
            first_apply: &grant,
            origin: RecoveryOrigin::Applying,
            restoration_started: true,
        }
    );
}

#[test]
fn recovery_views_retain_later_metadata_in_the_bound_inputs_without_reparsing() {
    let (_folder, store) = fixture();
    let binding = binding();
    let mut value = serde_json::to_value(first_apply(&binding)).unwrap();
    value["future_receipt_context"] = json!({"v": 2});
    value["preimages"]["future_format_note"] = json!("nonsemantic metadata");
    let grant: FirstApply = serde_json::from_value(value).unwrap();
    let mut live = store.prepare(binding.clone()).unwrap();
    probe(&mut live);
    live.grant(grant.clone()).unwrap();
    drop(live);
    let mut saved = document(&store);
    saved["binding"]["future_build_note"] = json!("retained alongside the known identity");
    replace(&store, &saved);

    let recovery = store.recover(&binding).unwrap();
    let before = bytes(&store);
    let RecoveryInputs::RestoreApply {
        binding: retained,
        first_apply,
        ..
    } = recovery.recovery_inputs().unwrap()
    else {
        panic!("the retained grant was lost");
    };
    assert_eq!(
        serde_json::to_value(retained).unwrap()["future_build_note"],
        saved["binding"]["future_build_note"]
    );
    assert_eq!(first_apply, &grant);
    assert_eq!(
        serde_json::to_value(first_apply).unwrap()["preimages"]["future_format_note"],
        json!("nonsemantic metadata")
    );
    assert_eq!(bytes(&store), before);
}
