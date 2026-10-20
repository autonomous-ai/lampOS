use crate::support;
use lamp_surround::{atomic, release::*};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    time::Duration,
};
use support::*;
use tough::schema::{Root, Snapshot, Timestamp};
use url::Url;

/// Preserve the production struct's serialized field order while changing a
/// fixture record, then recompute its unkeyed corruption-detection checksum.
fn rewrite_trust(f: &Fixture, mut change: impl FnMut(String) -> String) {
    let path = f.config.state_dir.join("trust.json");
    let original = fs::read_to_string(&path).unwrap();
    let old: Value = serde_json::from_str(&original).unwrap();
    let modified = change(original);
    let start = modified.find("\"trust\":").unwrap() + 8;
    let inner = &modified[start..modified.len() - 1];
    let checksum = sha(inner.as_bytes());
    let encoded = modified.replacen(old["sha256"].as_str().unwrap(), &checksum, 1);
    atomic::write(&path, encoded.as_bytes(), 1024 * 1024).unwrap();
}

fn unfinished_metadata(f: &Fixture) {
    rewrite_trust(f, |encoded| {
        encoded.replace(
            "\"cache_metadata_captured\":true",
            "\"cache_metadata_captured\":false",
        )
    });
    atomic::write(&f.config.state_dir.join("verification-pending"), b"1", 1).unwrap();
}

#[tokio::test]
async fn verified_set_is_complete_immutable_hash_addressed_and_recoverable() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let verified = f.stage().await.unwrap();
    assert_eq!(verified.manifest(), &manifest);
    assert_eq!(
        verified.set_sha256(),
        sha(&serde_json::to_vec(&manifest).unwrap())
    );
    assert_eq!(verified.binaries().len(), 3);
    for binary in &manifest.binaries {
        let checked = verified.binary(&binary.name).unwrap();
        assert_eq!(
            checked.path(),
            f.config.release_dir.join(&binary.sha256).join(&binary.name)
        );
        assert_eq!(checked.sha256(), binary.sha256);
        assert_eq!(checked.length(), binary.length);
        assert_eq!(
            fs::metadata(checked.path()).unwrap().permissions().mode() & 0o777,
            0o500
        );
    }
    assert_eq!(
        recover_verified(&f.config, verified.set_sha256()).unwrap(),
        verified
    );
    assert_eq!(f.stage().await.unwrap(), verified);
}

#[tokio::test]
async fn no_provisioned_anchor_and_untrusted_anchor_both_refuse() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    fs::remove_file(&f.config.trusted_root).unwrap();
    assert!(f.stage().await.unwrap_err().0.contains("provisioned"));
    atomic::write(
        &f.config.trusted_root,
        &serde_json::to_vec(&root(1, &key(1), &[&key(2)], FUTURE)).unwrap(),
        32_768,
    )
    .unwrap();
    assert!(f.stage().await.is_err());
}

#[tokio::test]
async fn unknown_backwards_and_disagreeing_utc_refuse() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let mut now = time();
    now.synchronized = false;
    assert!(
        verify_and_stage(&f.config, now, "release.json")
            .await
            .is_err()
    );
    now = time();
    now.unix_seconds -= 3600;
    assert!(
        verify_and_stage(&f.config, now, "release.json")
            .await
            .is_err()
    );
    f.stage().await.unwrap();
    now = time();
    let trust: Value =
        serde_json::from_slice(&fs::read(f.config.state_dir.join("trust.json")).unwrap()).unwrap();
    now.unix_seconds = trust["trust"]["latest_unix_seconds"].as_u64().unwrap() - 1;
    assert!(
        verify_and_stage(&f.config, now, "release.json")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn signatures_and_all_three_tuf_role_expiries_are_enforced() {
    for which in [
        "wrong-signature",
        "timestamp",
        "snapshot",
        "targets",
        "root",
    ] {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        let signer = if which == "wrong-signature" {
            key(2)
        } else {
            key(1)
        };
        f.publish(&manifest, 1, &signer, FUTURE);
        if which == "root" {
            atomic::write(
                &f.config.trusted_root,
                &serde_json::to_vec(&root(1, &key(1), &[&key(1)], EXPIRED)).unwrap(),
                32_768,
            )
            .unwrap();
        } else if which != "wrong-signature" {
            // Republish downstream hashes so expiry is the only failed check.
            if which == "targets" {
                let targets: Value =
                    serde_json::from_slice(&fs::read(f.metadata.join("targets.json")).unwrap())
                        .unwrap();
                let mut value = targets["signed"].clone();
                value["expires"] = json!(EXPIRED);
                f.publish_targets(value, 1, &key(1), FUTURE);
            } else if which == "snapshot" {
                let snapshot: Value =
                    serde_json::from_slice(&fs::read(f.metadata.join("snapshot.json")).unwrap())
                        .unwrap();
                let mut value = snapshot["signed"].clone();
                value["expires"] = json!(EXPIRED);
                let bytes = write_json(
                    &f.metadata.join("snapshot.json"),
                    &signed::<Snapshot>(value, &[&key(1)]),
                );
                write_json(
                    &f.metadata.join("timestamp.json"),
                    &signed::<Timestamp>(
                        json!({"_type":"timestamp","spec_version":"1.0.0","version":1,"expires":FUTURE,
                    "meta":{"snapshot.json":{"version":1,"length":bytes.len(),"hashes":{"sha256":sha(&bytes)}}}}),
                        &[&key(1)],
                    ),
                );
            } else {
                let timestamp: Value =
                    serde_json::from_slice(&fs::read(f.metadata.join("timestamp.json")).unwrap())
                        .unwrap();
                let mut value = timestamp["signed"].clone();
                value["expires"] = json!(EXPIRED);
                write_json(
                    &f.metadata.join("timestamp.json"),
                    &signed::<Timestamp>(value, &[&key(1)]),
                );
            }
        }
        assert!(f.stage().await.is_err(), "accepted {which}");
    }
}

#[tokio::test]
async fn manifest_and_binary_tampering_never_produce_a_descriptor() {
    for manifest_tamper in [false, true] {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        let path = if manifest_tamper {
            f.targets.join("release.json")
        } else {
            f.target(&manifest.binaries[1])
        };
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 1;
        fs::write(path, bytes).unwrap();
        assert!(f.stage().await.is_err());
        assert!(
            recover_verified(&f.config, &sha(&serde_json::to_vec(&manifest).unwrap())).is_err()
        );
    }
}

#[tokio::test]
async fn compatibility_names_counts_and_lengths_fail_closed() {
    for case in 0..10 {
        let f = Fixture::new();
        let mut manifest = f.manifest(1);
        match case {
            0 => manifest.board = "other".into(),
            1 => manifest.architecture = "x86_64".into(),
            2 => manifest.protocol = 2,
            3 => manifest.data_schema_min = 3,
            4 => manifest.data_schema_max = 1,
            5 => manifest.binaries.pop().map(|_| ()).unwrap(),
            6 => manifest.binaries[0].name = "../lamp-voice".into(),
            7 => manifest.binaries[1].name = "lamp-voice".into(),
            8 => manifest.binaries[0].length = 9_000,
            _ => manifest.release_sequence = 0,
        }
        f.publish(&manifest, 1, &key(1), FUTURE);
        assert!(f.stage().await.is_err(), "accepted case {case}");
    }
}

#[tokio::test]
async fn expired_feed_does_not_disable_previously_accepted_offline_recovery() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let accepted = f.stage().await.unwrap();
    f.publish(&manifest, 2, &key(1), EXPIRED);
    assert!(f.stage().await.is_err());
    fs::remove_dir_all(&f.metadata).unwrap();
    fs::remove_dir_all(&f.targets).unwrap();
    assert_eq!(
        recover_verified(&f.config, accepted.set_sha256()).unwrap(),
        accepted
    );
}

#[tokio::test]
async fn tuf_versions_release_sequence_and_same_sequence_substitution_are_fenced() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    let second = f.manifest(2);
    f.publish(&second, 2, &key(1), FUTURE);
    let new = f.stage().await.unwrap();
    f.publish(&first, 1, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
    f.publish(&first, 3, &key(1), FUTURE);
    assert!(f.stage().await.unwrap_err().0.contains("sequence"));
    let mut substitution = f.manifest(3);
    substitution.release_sequence = 2;
    f.publish(&substitution, 4, &key(1), FUTURE);
    assert!(f.stage().await.unwrap_err().0.contains("substitution"));
    assert!(recover_verified(&f.config, old.set_sha256()).is_ok());
    assert!(recover_verified(&f.config, new.set_sha256()).is_ok());
}

#[tokio::test]
async fn valid_root_rotation_survives_restart_and_old_key_revocation() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    write_json(
        &f.metadata.join("2.root.json"),
        &root(2, &key(2), &[&key(1), &key(2)], FUTURE),
    );
    let second = f.manifest(2);
    f.publish(&second, 2, &key(2), FUTURE);
    let new = f.stage().await.unwrap();
    fs::remove_file(f.metadata.join("2.root.json")).unwrap();
    assert_eq!(f.stage().await.unwrap(), new);
    f.publish(&second, 3, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
    assert!(recover_verified(&f.config, old.set_sha256()).is_ok());
}

#[tokio::test]
async fn rotation_requires_old_and_new_thresholds_and_consecutive_version() {
    for case in 0..3 {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(2), FUTURE);
        let version = if case == 2 { 3 } else { 2 };
        let signers = match case {
            0 => vec![key(1)],
            1 => vec![key(2)],
            _ => vec![key(1), key(2)],
        };
        let refs: Vec<_> = signers.iter().collect();
        write_json(
            &f.metadata.join("2.root.json"),
            &root(version, &key(2), &refs, FUTURE),
        );
        assert!(f.stage().await.is_err());
    }
}

#[tokio::test]
async fn incomplete_binary_staging_preserves_old_set_and_retries_without_replacing_it() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    let second = f.manifest(2);
    f.publish(&second, 2, &key(1), FUTURE);
    let missing = f.target(&second.binaries[1]);
    let saved = fs::read(&missing).unwrap();
    fs::remove_file(&missing).unwrap();
    assert!(f.stage().await.is_err());
    assert!(
        f.config
            .release_dir
            .join(&second.binaries[0].sha256)
            .join("lamp-voice")
            .is_file()
    );
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
    assert!(recover_verified(&f.config, &sha(&serde_json::to_vec(&second).unwrap())).is_err());
    f.publish(&first, 1, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
    f.publish(&second, 2, &key(1), FUTURE);
    write(&missing, &saved);
    let binary = &second.binaries[1];
    let dir = f.config.release_dir.join(&binary.sha256);
    atomic::ensure_private_dir(&dir).unwrap();
    atomic::write(&dir.join("lamp-body.partial"), b"interrupted", 128).unwrap();
    let new = f.stage().await.unwrap();
    assert_eq!(new.manifest(), &second);
    assert!(!dir.join("lamp-body.partial").exists());
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
}

#[tokio::test]
async fn corrupt_trust_or_missing_initialization_marker_never_silently_reset() {
    for missing in [false, true] {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        let accepted = f.stage().await.unwrap();
        if missing {
            fs::remove_file(f.config.state_dir.join("initialized")).unwrap();
        } else {
            atomic::write(&f.config.state_dir.join("trust.json"), b"{}", 1024).unwrap();
        }
        assert!(f.stage().await.is_err());
        assert!(recover_verified(&f.config, accepted.set_sha256()).is_err());
    }
}

#[tokio::test]
async fn offline_recovery_rehashes_and_rejects_writable_or_missing_binaries() {
    for case in 0..3 {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        let accepted = f.stage().await.unwrap();
        let binary = accepted.binary("lamp-body").unwrap().path();
        if case == 0 {
            fs::remove_file(binary).unwrap();
        } else {
            fs::set_permissions(binary, fs::Permissions::from_mode(0o700)).unwrap();
            if case == 2 {
                fs::write(binary, b"corrupt").unwrap();
            }
        }
        assert!(recover_verified(&f.config, accepted.set_sha256()).is_err());
    }
}

#[tokio::test]
async fn repository_and_installed_symlinks_are_never_followed() {
    for installed in [false, true] {
        let f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        let binary = &manifest.binaries[0];
        if installed {
            symlink(&f.targets, f.config.release_dir.join(&binary.sha256)).unwrap();
        } else {
            let path = f.target(binary);
            fs::remove_file(&path).unwrap();
            symlink(f.targets.join("release.json"), path).unwrap();
        }
        assert!(f.stage().await.is_err());
    }
}

#[tokio::test]
async fn stage_disk_obstruction_is_reported_and_never_acknowledged() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let dir = f.config.release_dir.join(&manifest.binaries[0].sha256);
    atomic::ensure_private_dir(&dir).unwrap();
    fs::create_dir(dir.join("lamp-voice.partial")).unwrap();
    assert!(f.stage().await.is_err());
    assert!(!dir.join("lamp-voice").exists());
    fs::remove_dir(dir.join("lamp-voice.partial")).unwrap();
    f.stage().await.unwrap();
}

#[tokio::test]
async fn retained_set_and_binary_directory_bounds_do_not_erase_rollback_releases() {
    for sets in [false, true] {
        let mut f = Fixture::new();
        if sets {
            f.config.limits.retained_sets = 2;
        } else {
            f.config.limits.binary_directories = 3;
        }
        let first = f.manifest(1);
        f.publish(&first, 1, &key(1), FUTURE);
        let accepted = f.stage().await.unwrap();
        let second = f.manifest(2);
        f.publish(&second, 2, &key(1), FUTURE);
        if sets {
            f.stage().await.unwrap();
            let third = f.manifest(3);
            f.publish(&third, 3, &key(1), FUTURE);
        }
        assert!(f.stage().await.is_err());
        assert_eq!(
            recover_verified(&f.config, accepted.set_sha256()).unwrap(),
            accepted
        );
    }
}

#[tokio::test]
async fn insecure_urls_credentials_queries_and_manifest_paths_are_rejected() {
    for url in [
        "http://example.test/",
        "https://user:secret@example.test/",
        "https://example.test/?token=secret",
        "https://example.test/#secret",
    ] {
        let mut f = Fixture::new();
        f.config.metadata_url = Url::parse(url).unwrap();
        assert!(f.stage().await.is_err());
    }
    let f = Fixture::new();
    for name in ["../release.json", "folder/release.json", "%2e%2e", ""] {
        assert!(verify_and_stage(&f.config, time(), name).await.is_err());
    }
}

#[tokio::test]
async fn metadata_and_target_byte_caps_are_independent_of_signed_lengths() {
    for metadata in [false, true] {
        let mut f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        if metadata {
            let targets: Value =
                serde_json::from_slice(&fs::read(f.metadata.join("targets.json")).unwrap())
                    .unwrap();
            let mut value = targets["signed"].clone();
            value["padding"] = json!("x".repeat(4096));
            f.publish_targets(value, 1, &key(1), FUTURE);
            f.config.limits.metadata_bytes = 2048;
        } else {
            f.config.limits.manifest_bytes = 100;
        }
        assert!(f.stage().await.is_err());
    }
}

#[tokio::test]
async fn complete_pending_metadata_is_recovered_but_corrupt_partial_metadata_fails_closed() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let accepted = f.stage().await.unwrap();
    unfinished_metadata(&f);
    assert_eq!(f.stage().await.unwrap(), accepted);
    unfinished_metadata(&f);
    fs::write(
        f.config.state_dir.join("verification/timestamp.json"),
        b"{\"signed\":",
    )
    .unwrap();
    assert!(
        f.stage()
            .await
            .unwrap_err()
            .0
            .contains("corrupt metadata cache")
    );
    assert_eq!(
        recover_verified(&f.config, accepted.set_sha256()).unwrap(),
        accepted
    );
}

#[tokio::test]
async fn accepted_feed_identity_and_provisioned_anchor_cannot_silently_change() {
    let mut f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let accepted = f.stage().await.unwrap();
    let original = f.config.metadata_url.clone();
    f.config.metadata_url = Url::parse("https://example.test/metadata/").unwrap();
    f.config.targets_url = Url::parse("https://example.test/targets/").unwrap();
    assert!(recover_verified(&f.config, accepted.set_sha256()).is_err());
    f.config.metadata_url = original;
    f.config.targets_url = Url::from_directory_path(&f.targets).unwrap();
    atomic::write(
        &f.config.trusted_root,
        &serde_json::to_vec(&root(2, &key(2), &[&key(2)], FUTURE)).unwrap(),
        32_768,
    )
    .unwrap();
    assert!(recover_verified(&f.config, accepted.set_sha256()).is_err());
}

#[tokio::test]
async fn root_rotation_is_retained_when_later_metadata_fails() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    write_json(
        &f.metadata.join("2.root.json"),
        &root(2, &key(2), &[&key(1), &key(2)], FUTURE),
    );
    let second = f.manifest(2);
    f.publish(&second, 2, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
    // Crash after the metadata/phase commit but before pending cleanup. The
    // retired seeded roles must not be imported a second time on restart.
    atomic::write(&f.config.state_dir.join("verification-pending"), b"1", 1).unwrap();
    fs::remove_file(f.metadata.join("2.root.json")).unwrap();
    f.publish(&second, 2, &key(2), FUTURE);
    f.stage().await.unwrap();
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
}

#[tokio::test]
async fn interrupted_root_cache_reconstructs_consecutive_signed_proofs() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    let cache = f.config.state_dir.join("verification");
    atomic::write(
        &cache.join("proof-2.json"),
        &serde_json::to_vec(&root(2, &key(2), &[&key(1), &key(2)], FUTURE)).unwrap(),
        32_768,
    )
    .unwrap();
    fs::remove_file(cache.join("root.json")).unwrap();
    unfinished_metadata(&f);
    let second = f.manifest(2);
    f.publish(&second, 2, &key(2), FUTURE);
    assert!(f.stage().await.is_ok());
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
}

#[tokio::test]
async fn malformed_rotation_does_not_poison_the_last_trusted_root() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    write_json(
        &f.metadata.join("2.root.json"),
        &root(2, &key(2), &[&key(2)], FUTURE),
    );
    assert!(f.stage().await.is_err());
    fs::remove_file(f.metadata.join("2.root.json")).unwrap();
    assert_eq!(f.stage().await.unwrap(), old);
}

#[tokio::test]
async fn target_count_request_count_and_deadline_are_bounded() {
    for case in 0..3 {
        let mut f = Fixture::new();
        let manifest = f.manifest(1);
        f.publish(&manifest, 1, &key(1), FUTURE);
        if case == 0 {
            f.config.limits.target_count = 4;
            let targets: Value =
                serde_json::from_slice(&fs::read(f.metadata.join("targets.json")).unwrap())
                    .unwrap();
            let mut value = targets["signed"].clone();
            value["targets"]["unused"] = json!({"length":1,"hashes":{"sha256":sha(b"x")}});
            f.publish_targets(value, 1, &key(1), FUTURE);
        } else if case == 1 {
            f.config.limits.requests = 8;
            write_json(
                &f.metadata.join("2.root.json"),
                &root(2, &key(2), &[&key(1), &key(2)], FUTURE),
            );
            f.publish(&manifest, 1, &key(2), FUTURE);
        } else {
            f.config.limits.download_timeout = Duration::from_nanos(1);
        }
        assert!(f.stage().await.is_err(), "accepted cap case {case}");
    }
}

#[tokio::test]
async fn delegated_targets_are_rejected_before_any_delegated_fetch_or_cache_write() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let targets: Value =
        serde_json::from_slice(&fs::read(f.metadata.join("targets.json")).unwrap()).unwrap();
    let mut value = targets["signed"].clone();
    value["delegations"] = json!({"keys":{},"roles":[]});
    f.publish_targets(value, 1, &key(1), FUTURE);
    assert!(f.stage().await.is_err());
}

#[tokio::test]
async fn overflowing_root_version_is_rejected_before_library_arithmetic() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    atomic::write(
        &f.config.trusted_root,
        &serde_json::to_vec(&root(u64::MAX, &key(1), &[&key(1)], FUTURE)).unwrap(),
        32_768,
    )
    .unwrap();
    assert!(f.stage().await.unwrap_err().0.contains("too large"));
}

#[tokio::test]
async fn persistent_checksum_cannot_replace_signed_manifest_authentication() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let accepted = f.stage().await.unwrap();
    let mut altered = manifest.clone();
    altered.protocol = 7;
    let old_json = serde_json::to_string(&manifest).unwrap();
    let new_json = serde_json::to_string(&altered).unwrap();
    let new_set = sha(new_json.as_bytes());
    rewrite_trust(&f, |encoded| {
        encoded
            .replace(
                &serde_json::to_string(&old_json).unwrap(),
                &serde_json::to_string(&new_json).unwrap(),
            )
            .replace(
                &format!("\"highest_set_sha256\":\"{}\"", accepted.set_sha256()),
                &format!("\"highest_set_sha256\":\"{new_set}\""),
            )
            .replace(
                &format!("\"accepted\":{{\"{}\":", accepted.set_sha256()),
                &format!("\"accepted\":{{\"{new_set}\":"),
            )
    });
    let error = recover_verified(&f.config, &new_set).unwrap_err();
    assert!(
        error.0.contains("signed target length or SHA-256 differs"),
        "{error}"
    );
}

#[tokio::test]
async fn hardlinked_repository_target_and_symlink_ancestor_are_rejected() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    fs::hard_link(f.target(&manifest.binaries[0]), f.targets.join("alias")).unwrap();
    assert!(f.stage().await.is_err());
    fs::remove_file(f.targets.join("alias")).unwrap();
    let parent = f.config.state_dir.parent().unwrap();
    symlink(&f.metadata, parent.join("metadata-link")).unwrap();
    let mut config = f.config.clone();
    config.metadata_url = Url::from_directory_path(parent.join("metadata-link")).unwrap();
    config.state_dir = parent.join("other-state");
    assert!(
        verify_and_stage(&config, time(), "release.json")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn later_tuf_observed_utc_survives_restart_even_without_an_accepted_update() {
    let f = Fixture::new();
    let manifest = f.manifest(1);
    f.publish(&manifest, 1, &key(1), FUTURE);
    let accepted = f.stage().await.unwrap();
    atomic::write(&f.config.state_dir.join("verification-pending"), b"1", 1).unwrap();
    atomic::write(
        &f.config
            .state_dir
            .join("verification/latest_known_time.json"),
        b"\"2099-01-01T00:00:00Z\"",
        128,
    )
    .unwrap();
    assert!(f.stage().await.is_err());
    assert!(f.stage().await.unwrap_err().0.contains("UTC"));
    assert_eq!(
        recover_verified(&f.config, accepted.set_sha256()).unwrap(),
        accepted
    );
}

#[tokio::test]
async fn timestamp_key_rotation_resets_fast_forward_floors_even_when_an_old_key_remains() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 50, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    let new_root = root(2, &key(2), &[&key(1), &key(2)], FUTURE);
    let mut value = new_root["signed"].clone();
    let old_id = key_id(&key(1));
    let new_id = key_id(&key(2));
    value["keys"][&old_id] = key_value(&key(1));
    for role in ["timestamp", "snapshot", "targets"] {
        value["roles"][role]["keyids"] = json!([old_id, new_id]);
    }
    write_json(
        &f.metadata.join("2.root.json"),
        &signed::<Root>(value, &[&key(1), &key(2)]),
    );
    let second = f.manifest(2);
    f.publish(&second, 1, &key(3), FUTURE);
    assert!(f.stage().await.is_err());
    atomic::write(&f.config.state_dir.join("verification-pending"), b"1", 1).unwrap();
    f.publish(&second, 1, &key(2), FUTURE);
    f.stage().await.unwrap();
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
}

#[tokio::test]
async fn cancellation_during_binary_staging_keeps_the_clock_recovery_journal() {
    let f = Fixture::new();
    let first = f.manifest(1);
    f.publish(&first, 1, &key(1), FUTURE);
    let old = f.stage().await.unwrap();
    let second = f.manifest(2);
    f.publish(&second, 2, &key(1), FUTURE);
    let first_binary = f
        .config
        .release_dir
        .join(&second.binaries[0].sha256)
        .join(&second.binaries[0].name);
    let config = f.config.clone();
    let task = tokio::spawn(async move { verify_and_stage(&config, time(), "release.json").await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !first_binary.exists() && !task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(first_binary.exists());
    assert!(
        !task.is_finished(),
        "fixture must interrupt before all three binaries commit"
    );
    assert!(
        f.config.state_dir.join("verification-pending").is_file(),
        "target reads still update Tough's UTC cache"
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(recover_verified(&f.config, old.set_sha256()).unwrap(), old);
    assert!(recover_verified(&f.config, &sha(&serde_json::to_vec(&second).unwrap())).is_err());
}
