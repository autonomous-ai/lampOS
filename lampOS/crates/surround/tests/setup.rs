use lamp_surround::{atomic, setup::*};
use std::{fs, net::IpAddr, os::unix::fs::PermissionsExt};

fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}
fn store(dir: &tempfile::TempDir) -> SetupStore {
    SetupStore::open(&dir.path().join("setup.json"), "boot-a", 0).unwrap()
}
fn lan(kind: LinkKind) -> NetworkEvidence {
    NetworkEvidence {
        kind,
        link_up: true,
        address: Some("192.168.7.42".parse().unwrap()),
        addressing: Addressing::DhcpBound,
        setup_ap: false,
        cloud: Reachability::Unknown,
        authentication_rejected: false,
    }
}
fn receipt(store: &SetupStore, seq: u64, at: u64, value: ReceiptValue) -> Receipt {
    Receipt {
        attempt_id: store.progress().attempt_id.clone().unwrap(),
        generation: store.progress().generation,
        clock_id: store.progress().clock_id.clone(),
        sequence: seq,
        observed_us: at,
        value,
    }
}
fn apply(store: &mut SetupStore, seq: u64, at: u64, value: ReceiptValue) -> bool {
    let r = receipt(store, seq, at, value);
    store.observe(at, r).unwrap()
}

#[test]
fn wired_is_verified_and_empty_address_or_ap_never_means_lan() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("wired", 0, SetupMode::Wired).unwrap();
    assert!(!apply(
        &mut s,
        1,
        1,
        ReceiptValue::Network(lan(LinkKind::Wifi))
    ));
    let mut e = lan(LinkKind::Wired);
    e.link_up = false;
    apply(&mut s, 1, 1, ReceiptValue::Network(e.clone()));
    assert_eq!(s.progress().phase, Phase::Connecting);
    e.link_up = true;
    e.address = None;
    apply(&mut s, 2, 2, ReceiptValue::Network(e.clone()));
    assert_eq!(s.progress().phase, Phase::Connecting);
    e = lan(LinkKind::Wired);
    e.setup_ap = true;
    apply(&mut s, 3, 3, ReceiptValue::Network(e));
    assert_eq!(s.progress().phase, Phase::Connecting);
    apply(&mut s, 4, 4, ReceiptValue::Network(lan(LinkKind::Wired)));
    assert_eq!(s.progress().phase, Phase::LanConnected);
    assert!(!s.progress().ready());
}

#[test]
fn address_is_persisted_before_cloud_and_runtime_and_survives_runtime_failure() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("wifi", 0, SetupMode::Wifi).unwrap();
    apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    let bytes = atomic::read(&dir.path().join("setup.json"), MAX_PROGRESS_BYTES)
        .unwrap()
        .unwrap();
    let p: Progress = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        p.network.unwrap().address,
        Some("192.168.7.42".parse::<IpAddr>().unwrap())
    );
    assert!(!p.runtime_fresh);
    apply(&mut s, 1, 2, ReceiptValue::Runtime(RuntimeState::Failed));
    assert_eq!(s.progress().failure, Some(Failure::RuntimeFailed));
    assert!(s.progress().network_fresh);
    assert!(s.progress().network.as_ref().unwrap().lan_ready());
    assert!(!apply(
        &mut s,
        2,
        3,
        ReceiptValue::Runtime(RuntimeState::Ready)
    ));
}

#[test]
fn runtime_ready_requires_fresh_selected_lan_and_not_cloud_alone() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    assert!(!apply(
        &mut s,
        1,
        1,
        ReceiptValue::Runtime(RuntimeState::Ready)
    ));
    let mut e = lan(LinkKind::Wifi);
    e.cloud = Reachability::Reachable;
    e.addressing = Addressing::DhcpPending;
    apply(&mut s, 1, 1, ReceiptValue::Network(e));
    assert!(!apply(
        &mut s,
        1,
        2,
        ReceiptValue::Runtime(RuntimeState::Ready)
    ));
    apply(&mut s, 2, 2, ReceiptValue::Network(lan(LinkKind::Wifi)));
    apply(&mut s, 1, 3, ReceiptValue::Runtime(RuntimeState::Ready));
    assert!(s.progress().ready());
    s.tick(RECEIPT_MAX_AGE_US + 4).unwrap();
    assert!(!s.progress().ready());
}

#[test]
fn supersession_and_reused_attempt_id_cannot_adopt_old_result() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("same", 0, SetupMode::Wifi).unwrap();
    let old = receipt(&s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    s.begin("same", 1, SetupMode::Wifi).unwrap();
    assert!(!s.observe(2, old).unwrap());
    assert_eq!(s.progress().generation, 2);
    assert_eq!(s.progress().phase, Phase::Connecting);
}

#[test]
fn backwards_future_stale_and_other_boot_receipts_do_not_advance_sequences() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    let mut r = receipt(&s, 1, 2, ReceiptValue::Network(lan(LinkKind::Wifi)));
    assert!(!s.observe(1, r.clone()).unwrap());
    r.observed_us = 0;
    assert!(!s.observe(RECEIPT_MAX_AGE_US + 1, r.clone()).unwrap());
    r.observed_us = RECEIPT_MAX_AGE_US + 1;
    r.clock_id = "other".into();
    assert!(!s.observe(RECEIPT_MAX_AGE_US + 1, r).unwrap());
    assert_eq!(s.progress().network_sequence, 0);
    assert!(s.tick(1).is_err());
    let at = RECEIPT_MAX_AGE_US + 2;
    assert!(apply(
        &mut s,
        1,
        at,
        ReceiptValue::Network(lan(LinkKind::Wifi))
    ));
    assert!(!apply(
        &mut s,
        1,
        at + 1,
        ReceiptValue::Network(lan(LinkKind::Wifi))
    ));
}

#[test]
fn wrong_key_needs_two_distinct_sequenced_observations() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    let mut e = lan(LinkKind::Wifi);
    e.link_up = false;
    e.authentication_rejected = true;
    assert!(apply(&mut s, 1, 1, ReceiptValue::Network(e.clone())));
    assert_eq!(s.progress().phase, Phase::Connecting);
    assert!(!apply(&mut s, 1, 2, ReceiptValue::Network(e.clone())));
    assert!(apply(&mut s, 2, 3, ReceiptValue::Network(e)));
    assert_eq!(s.progress().failure, Some(Failure::WrongKey));
    assert!(!apply(
        &mut s,
        3,
        4,
        ReceiptValue::Network(lan(LinkKind::Wifi))
    ));
}

#[test]
fn elapsed_deadline_wins_over_a_late_success_even_without_ticks() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    assert!(!apply(
        &mut s,
        1,
        JOIN_TIMEOUT_US,
        ReceiptValue::Network(lan(LinkKind::Wifi))
    ));
    assert_eq!(s.progress().failure, Some(Failure::JoinTimeout));
    s.begin("b", JOIN_TIMEOUT_US + 1, SetupMode::Wifi).unwrap();
    apply(
        &mut s,
        1,
        JOIN_TIMEOUT_US + 2,
        ReceiptValue::Network(lan(LinkKind::Wifi)),
    );
    let deadline = s.progress().deadline_us.unwrap();
    s.tick(deadline).unwrap();
    assert_eq!(s.progress().failure, Some(Failure::RuntimeTimeout));
    assert!(!apply(
        &mut s,
        1,
        deadline,
        ReceiptValue::Runtime(RuntimeState::Ready)
    ));
}

#[test]
fn preparation_heartbeats_cannot_extend_runtime_deadline() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    let deadline = s.progress().deadline_us;
    for n in 1..5 {
        apply(
            &mut s,
            n,
            n + 1,
            ReceiptValue::Runtime(RuntimeState::Preparing),
        );
    }
    assert_eq!(s.progress().deadline_us, deadline);
}

#[test]
fn a_new_preparing_or_unknown_receipt_withdraws_ready_without_reopening_setup() {
    for state in [RuntimeState::Preparing, RuntimeState::Unknown] {
        let dir = private_dir();
        let mut s = store(&dir);
        s.begin("a", 0, SetupMode::Wifi).unwrap();
        apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
        apply(&mut s, 1, 2, ReceiptValue::Runtime(RuntimeState::Ready));
        assert!(apply(&mut s, 2, 3, ReceiptValue::Runtime(state)));
        assert!(!s.progress().ready());
        assert_eq!(s.progress().phase, Phase::Ready);
        assert_eq!(s.progress().deadline_us, None);
        assert_eq!(s.progress().runtime, state);
        assert!(!s.progress().runtime_fresh);
        assert_eq!(s.progress().runtime_sequence, 2);
        assert!(s.progress().network_fresh);
        assert!(!apply(
            &mut s,
            1,
            4,
            ReceiptValue::Runtime(RuntimeState::Ready)
        ));
        assert!(apply(
            &mut s,
            3,
            4,
            ReceiptValue::Runtime(RuntimeState::Ready)
        ));
        assert!(s.progress().ready());
    }
}

#[test]
fn failed_probe_withdrawals_are_durable_separate_and_do_not_write_on_every_tick() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    apply(&mut s, 1, 2, ReceiptValue::Runtime(RuntimeState::Ready));
    let address = s.progress().network.clone();
    s.withdraw_runtime(3).unwrap();
    assert!(!s.progress().ready());
    assert!(s.progress().network_fresh);
    assert_eq!(s.progress().network, address);
    assert_eq!(s.progress().phase, Phase::Ready);
    let disk: Progress =
        serde_json::from_slice(&fs::read(dir.path().join("setup.json")).unwrap()).unwrap();
    assert_eq!(&disk, s.progress());
    fs::create_dir(dir.path().join(".setup.json.pending")).unwrap();
    s.withdraw_runtime(4).unwrap();
    assert_eq!(
        s.progress().updated_us,
        3,
        "unchanged withdrawal does not fsync"
    );
    assert!(
        s.withdraw_runtime(3).is_err(),
        "even a no-op advances the caller clock guard"
    );
    assert!(s.withdraw_network(5).is_err());
    assert!(
        s.progress().network_fresh,
        "failed storage cannot acknowledge the change"
    );
    fs::remove_dir(dir.path().join(".setup.json.pending")).unwrap();
    s.withdraw_network(5).unwrap();
    assert!(!s.progress().network_fresh);
    assert!(!s.progress().runtime_fresh);
    assert_eq!(s.progress().network, address);
    assert_eq!(s.progress().deadline_us, None);
    let disk: Progress =
        serde_json::from_slice(&fs::read(dir.path().join("setup.json")).unwrap()).unwrap();
    assert_eq!(&disk, s.progress());
}

#[test]
fn withdrawal_preserves_preparation_deadlines_and_expired_lan_cannot_revive_runtime() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    apply(&mut s, 1, 2, ReceiptValue::Runtime(RuntimeState::Preparing));
    let deadline = s.progress().deadline_us;
    s.withdraw_runtime(3).unwrap();
    s.withdraw_network(4).unwrap();
    assert_eq!(s.progress().phase, Phase::PreparingRuntime);
    assert_eq!(s.progress().deadline_us, deadline);
    assert!(!apply(
        &mut s,
        2,
        5,
        ReceiptValue::Runtime(RuntimeState::Ready)
    ));
    apply(&mut s, 2, 6, ReceiptValue::Network(lan(LinkKind::Wifi)));
    apply(&mut s, 2, 6, ReceiptValue::Runtime(RuntimeState::Ready));
    s.withdraw_network(7).unwrap();
    assert!(apply(
        &mut s,
        3,
        8,
        ReceiptValue::Runtime(RuntimeState::Preparing)
    ));
    assert!(!s.progress().ready());
    assert_eq!(s.progress().phase, Phase::Ready);
    assert!(!s.progress().runtime_fresh);
    assert!(!s.progress().network_fresh);
}

#[test]
fn restart_persists_interrupted_and_never_replays_active_or_ready_claims() {
    for ready in [false, true] {
        let dir = private_dir();
        let mut s = store(&dir);
        s.begin("a", 0, SetupMode::Wifi).unwrap();
        apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
        if ready {
            apply(&mut s, 1, 2, ReceiptValue::Runtime(RuntimeState::Ready));
        }
        drop(s);
        let restored = SetupStore::open(&dir.path().join("setup.json"), "boot-b", 0).unwrap();
        assert_eq!(restored.progress().phase, Phase::Interrupted);
        assert!(!restored.progress().ready());
        assert!(!restored.progress().network_fresh);
        let disk: Progress =
            serde_json::from_slice(&fs::read(dir.path().join("setup.json")).unwrap()).unwrap();
        assert_eq!(disk, *restored.progress());
    }
}

#[test]
fn failed_write_does_not_acknowledge_or_change_visible_progress() {
    let dir = private_dir();
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    let before = s.progress().clone();
    fs::create_dir(dir.path().join(".setup.json.pending")).unwrap();
    let r = receipt(&s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    assert!(s.observe(1, r).is_err());
    assert_eq!(s.progress(), &before);
    assert!(s.begin("b", 2, SetupMode::Wifi).is_err());
    assert_eq!(s.progress(), &before);
}

#[test]
fn corrupt_unknown_and_inconsistent_records_fail_closed() {
    let dir = private_dir();
    let s = store(&dir);
    let mut value = serde_json::to_value(s.progress()).unwrap();
    for mutation in ["unknown", "version", "time"] {
        let mut v = value.clone();
        match mutation {
            "unknown" => v["secret"] = "never-echo".into(),
            "version" => v["schema_version"] = 2.into(),
            _ => {
                v["started_us"] = 100.into();
                v["updated_us"] = 1.into();
            }
        }
        atomic::write(
            &dir.path().join("setup.json"),
            &serde_json::to_vec(&v).unwrap(),
            MAX_PROGRESS_BYTES,
        )
        .unwrap();
        assert!(SetupStore::open(&dir.path().join("setup.json"), "boot-a", 5).is_err());
    }
    value["phase"] = "unknown-phase".into();
    atomic::write(
        &dir.path().join("setup.json"),
        &serde_json::to_vec(&value).unwrap(),
        MAX_PROGRESS_BYTES,
    )
    .unwrap();
    assert!(SetupStore::open(&dir.path().join("setup.json"), "boot-a", 5).is_err());
}

#[test]
fn local_only_and_ap_addresses_cannot_be_readiness() {
    for address in [
        "127.0.0.1",
        "0.0.0.0",
        "169.254.1.1",
        "255.255.255.255",
        "224.0.0.1",
        "::1",
        "fe80::1",
    ] {
        assert!(!usable_address(address.parse().unwrap()));
    }
    assert!(usable_address("192.168.1.2".parse().unwrap()));
    assert!(usable_address("fd01::2".parse().unwrap()));
}

/// #1505: setup's progress as a later build writes it, with a field this build
/// doesn't know. The same file without it reopens; with it, it is refused
/// today and lamp-surround doesn't start. No fix yet, and no unit starts
/// lamp-surround on a lamp: whoever makes it read one release forward flips
/// this to a plain test (Tests).
#[test]
#[should_panic(expected = "invalid setup record")]
fn progress_from_a_later_build_is_read() {
    let dir = private_dir();
    let path = dir.path().join("setup.json");
    let mut s = store(&dir);
    s.begin("a", 0, SetupMode::Wifi).unwrap();
    apply(&mut s, 1, 1, ReceiptValue::Network(lan(LinkKind::Wifi)));
    drop(s);
    let restored = SetupStore::open(&path, "boot-b", 0).unwrap();
    let phase = restored.progress().phase;
    drop(restored);
    let mut later: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    later["a_field_from_a_later_build"] = serde_json::json!(true);
    atomic::write(
        &path,
        &serde_json::to_vec(&later).unwrap(),
        MAX_PROGRESS_BYTES,
    )
    .unwrap();
    let read = SetupStore::open(&path, "boot-c", 0).unwrap();
    assert_eq!(read.progress().phase, phase);
}
