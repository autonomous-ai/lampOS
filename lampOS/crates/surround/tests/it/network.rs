use lamp_surround::{
    config::Secret,
    network::*,
    setup::{Addressing, LinkKind, NetworkEvidence, RECEIPT_MAX_AGE_US, Reachability},
};
use std::collections::VecDeque;

fn selection() -> Selection {
    Selection {
        enabled: true,
        interface: "wlan0".into(),
        kind: LinkKind::Wifi,
        recovery_profile_uuid: Some("11111111-1111-4111-8111-111111111111".into()),
        station_profile_uuid: Some("22222222-2222-4222-8222-222222222222".into()),
    }
}
fn request(generation: u64, sequence: u64, command: NetworkCommand) -> NetworkRequest {
    NetworkRequest {
        id: OperationId {
            attempt_id: format!("attempt-{generation}"),
            generation,
            sequence,
        },
        command,
    }
}
fn wifi() -> WifiCredentials {
    WifiCredentials::new(
        b"private-network-name".to_vec(),
        WifiSecurity::WpaPsk(Secret::new(b"private-password".to_vec()).unwrap()),
    )
    .unwrap()
}
fn evidence(ready: bool) -> NetworkEvidence {
    NetworkEvidence {
        kind: LinkKind::Wifi,
        link_up: ready,
        address: ready.then(|| "192.168.1.50".parse().unwrap()),
        addressing: Addressing::DhcpBound,
        setup_ap: false,
        cloud: Reachability::Unknown,
        authentication_rejected: false,
    }
}
fn lost(ap_active: bool, clients: Option<u16>) -> RecoveryObservation {
    RecoveryObservation {
        link: LinkStatus::Lost,
        ap_active,
        ap_clients: clients,
        saved_station: true,
    }
}

#[derive(Default)]
struct Bus {
    calls: Vec<String>,
    observations: VecDeque<Result<NetworkEvidence, NetworkError>>,
    fail_join: bool,
    fail_rollback: bool,
    fail_save_after_effect: bool,
    fail_destroy_checkpoint: bool,
    saved_profiles: Vec<String>,
    expired_checkpoint: bool,
    bad_device: bool,
}
impl ManagerBus for Bus {
    fn selected_device(&mut self, selection: &Selection) -> Result<String, NetworkError> {
        self.calls.push(format!("device:{}", selection.interface));
        Ok(if self.bad_device {
            "/"
        } else {
            "/device/wifi0"
        }
        .into())
    }
    fn checkpoint(&mut self, device: &str, timeout: u32) -> Result<String, NetworkError> {
        self.calls.push(format!("checkpoint:{device}:{timeout}"));
        Ok("/checkpoint/one".into())
    }
    fn join(
        &mut self,
        device: &str,
        _: &WifiCredentials,
        _: &OperationId,
    ) -> Result<String, NetworkError> {
        self.calls.push(format!("join:{device}"));
        if self.fail_join {
            Err(NetworkError::Unavailable)
        } else {
            Ok("/profile/candidate".into())
        }
    }
    fn observe(
        &mut self,
        _: &str,
        _: &Selection,
        expected: Option<&str>,
    ) -> Result<NetworkEvidence, NetworkError> {
        self.calls
            .push(format!("observe:{}", expected.unwrap_or("none")));
        self.observations.pop_front().unwrap_or(Ok(evidence(false)))
    }
    fn save(&mut self, profile: &str) -> Result<(), NetworkError> {
        self.calls.push(format!("save:{profile}"));
        self.saved_profiles.push(profile.into());
        if self.fail_save_after_effect {
            Err(NetworkError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn destroy_checkpoint(&mut self, checkpoint: &str) -> Result<(), NetworkError> {
        self.calls.push(format!("commit:{checkpoint}"));
        if self.fail_destroy_checkpoint {
            Err(NetworkError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn rollback(&mut self, checkpoint: &str, device: &str) -> Result<(), NetworkError> {
        self.calls.push(format!("rollback:{checkpoint}:{device}"));
        if self.expired_checkpoint {
            return Err(NetworkError::CheckpointExpired);
        }
        if self.fail_rollback {
            Err(NetworkError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn activate_profile(
        &mut self,
        device: &str,
        _: &str,
        recovery: bool,
    ) -> Result<String, NetworkError> {
        self.calls.push(format!("activate:{device}:{recovery}"));
        Ok("/profile/saved".into())
    }
}

#[test]
fn input_bounds_and_explicit_selection_are_required() {
    assert!(WifiCredentials::new(vec![], WifiSecurity::Open).is_err());
    assert!(WifiCredentials::new(vec![b'a'; 33], WifiSecurity::Open).is_err());
    assert!(WifiCredentials::new(vec![b'a'; 32], WifiSecurity::Open).is_ok());
    for psk in [
        "short",
        "1234567\t",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef012g",
    ] {
        assert!(
            WifiCredentials::new(
                b"name".to_vec(),
                WifiSecurity::WpaPsk(Secret::new(psk.as_bytes().to_vec()).unwrap())
            )
            .is_err()
        );
    }
    let mut s = selection();
    s.enabled = false;
    assert_eq!(s.validate(), Err(NetworkError::Disabled));
    s.enabled = true;
    s.interface = "wlan0;reboot".into();
    assert_eq!(s.validate(), Err(NetworkError::InvalidRequest));
    s.interface = "wlan0".into();
    s.recovery_profile_uuid = Some("guessed-profile-name".into());
    assert_eq!(s.validate(), Err(NetworkError::InvalidRequest));
}

#[test]
fn unsupported_and_scripted_adapters_never_fake_activation_and_are_bounded() {
    let mut unavailable = UnsupportedAdapter::default();
    unavailable
        .submit(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        unavailable.submit(request(1, 2, NetworkCommand::Probe)),
        Err(NetworkError::Busy)
    );
    assert_eq!(
        unavailable.poll().unwrap().result,
        Err(NetworkError::Unsupported)
    );
    let steps = vec![ScriptStep {
        expected: CommandKind::Probe,
        observed_us: 20,
        result: Ok(NetworkResult::Observation(evidence(true))),
    }];
    let mut scripted = ScriptedAdapter::new(steps).unwrap();
    scripted
        .submit(request(1, 1, NetworkCommand::Probe))
        .unwrap();
    assert_eq!(
        scripted.submit(request(1, 2, NetworkCommand::Probe)),
        Err(NetworkError::Busy)
    );
    assert_eq!(scripted.poll().unwrap().observed_us, 20);
    assert_eq!(
        scripted.submit(request(1, 2, NetworkCommand::Probe)),
        Err(NetworkError::Protocol)
    );
    assert!(
        ScriptedAdapter::new(
            (0..=MAX_SCRIPT_STEPS)
                .map(|_| ScriptStep {
                    expected: CommandKind::Probe,
                    observed_us: 0,
                    result: Err(NetworkError::Unavailable)
                })
                .collect()
        )
        .is_err()
    );
}

#[test]
fn continuous_known_loss_uses_elapsed_90_seconds_not_a_probe_count() {
    let mut r = Recovery::new("boot", 0).unwrap();
    for second in 0..90 {
        assert_eq!(
            r.observe(
                "boot",
                second + 1,
                second * 1_000_000,
                second * 1_000_000,
                lost(false, None)
            )
            .unwrap(),
            None
        );
    }
    assert_eq!(
        r.observe("boot", 91, LOSS_GRACE_US, LOSS_GRACE_US, lost(false, None))
            .unwrap(),
        Some(RecoveryAction::StartAp)
    );
    assert_eq!(r.tick(LOSS_GRACE_US + 1).unwrap(), None);
}

#[test]
fn probe_gaps_and_unknown_link_do_not_prove_continuous_loss() {
    let mut r = Recovery::new("boot", 0).unwrap();
    r.observe("boot", 1, 0, 0, lost(false, None)).unwrap();
    assert_eq!(
        r.observe("boot", 2, LOSS_GRACE_US, LOSS_GRACE_US, lost(false, None))
            .unwrap(),
        None
    );
    let mut unknown = lost(false, None);
    unknown.link = LinkStatus::Unknown;
    assert_eq!(
        r.observe("boot", 3, LOSS_GRACE_US + 1, LOSS_GRACE_US + 1, unknown)
            .unwrap(),
        None
    );
    assert_eq!(r.tick(2 * LOSS_GRACE_US).unwrap(), None);
}

#[test]
fn ap_retry_waits_120_seconds_and_never_disconnects_clients_or_unknown_count() {
    for clients in [Some(1), None] {
        let mut r = Recovery::new("boot", 0).unwrap();
        r.observe("boot", 1, 0, 0, lost(true, clients)).unwrap();
        assert_eq!(
            r.observe(
                "boot",
                2,
                RETRY_INTERVAL_US,
                RETRY_INTERVAL_US,
                lost(true, clients)
            )
            .unwrap(),
            None
        );
        assert_eq!(
            r.observe(
                "boot",
                3,
                RETRY_INTERVAL_US + 1,
                RETRY_INTERVAL_US + 1,
                lost(true, Some(0))
            )
            .unwrap(),
            Some(RecoveryAction::RetrySaved)
        );
    }
    let mut r = Recovery::new("boot", 0).unwrap();
    r.observe("boot", 1, 0, 0, lost(true, Some(0))).unwrap();
    assert_eq!(
        r.observe(
            "boot",
            2,
            RETRY_INTERVAL_US - 1,
            RETRY_INTERVAL_US - 1,
            lost(true, Some(0))
        )
        .unwrap(),
        None
    );
    assert_eq!(
        r.tick(RETRY_INTERVAL_US).unwrap(),
        Some(RecoveryAction::RetrySaved)
    );
}

#[test]
fn own_join_restores_ap_at_45_seconds_even_when_every_later_probe_fails() {
    let mut r = Recovery::new("boot", 0).unwrap();
    r.observe("boot", 1, 0, 0, lost(true, Some(0))).unwrap();
    r.observe(
        "boot",
        2,
        RETRY_INTERVAL_US,
        RETRY_INTERVAL_US,
        lost(true, Some(0)),
    )
    .unwrap();
    assert_eq!(
        r.tick(RETRY_INTERVAL_US + JOIN_TIMEOUT_US - 1).unwrap(),
        None
    );
    assert_eq!(
        r.tick(RETRY_INTERVAL_US + JOIN_TIMEOUT_US).unwrap(),
        Some(RecoveryAction::RestoreAp)
    );
    assert_eq!(
        r.tick(RETRY_INTERVAL_US + JOIN_TIMEOUT_US + 1).unwrap(),
        None
    );
}

#[test]
fn local_lan_success_cancels_recovery_even_without_cloud() {
    let mut r = Recovery::new("boot", 0).unwrap();
    r.observe("boot", 1, 0, 0, lost(true, Some(0))).unwrap();
    r.observe(
        "boot",
        2,
        RETRY_INTERVAL_US,
        RETRY_INTERVAL_US,
        lost(true, Some(0)),
    )
    .unwrap();
    let mut healthy = lost(false, None);
    healthy.link = LinkStatus::LanReady;
    r.observe(
        "boot",
        3,
        RETRY_INTERVAL_US + 1,
        RETRY_INTERVAL_US + 1,
        healthy,
    )
    .unwrap();
    assert_eq!(r.tick(RETRY_INTERVAL_US + JOIN_TIMEOUT_US).unwrap(), None);
}

#[test]
fn recovery_rejects_stale_reordered_other_boot_and_backwards_time() {
    let mut r = Recovery::new("boot", 0).unwrap();
    r.observe("boot", 1, 0, 0, lost(false, None)).unwrap();
    assert_eq!(
        r.observe("other", 2, 1, 1, lost(false, None)),
        Err(NetworkError::Stale)
    );
    assert_eq!(
        r.observe("boot", 1, 1, 1, lost(false, None)),
        Err(NetworkError::Stale)
    );
    assert_eq!(
        r.observe("boot", 2, 2, 1, lost(false, None)),
        Err(NetworkError::Stale)
    );
    assert_eq!(
        r.observe("boot", 2, 0, RECEIPT_MAX_AGE_US + 1, lost(false, None)),
        Err(NetworkError::Stale)
    );
    r.tick(10).unwrap();
    assert_eq!(r.tick(9), Err(NetworkError::ClockReversed));
}

#[test]
fn wifi_mutation_checkpoints_only_the_selected_device_and_commits_only_its_profile() {
    let bus = Bus {
        observations: [Ok(evidence(true))].into(),
        ..Bus::default()
    };
    let mut session = ManagerSession::new(selection(), bus).unwrap();
    assert_eq!(
        session.execute(request(1, 1, NetworkCommand::JoinWifi(wifi()))),
        Ok(NetworkResult::Accepted)
    );
    assert_eq!(
        session.execute(request(1, 2, NetworkCommand::Commit)),
        Ok(NetworkResult::Committed)
    );
    assert_eq!(
        session.bus().calls,
        [
            "device:wlan0",
            "checkpoint:/device/wifi0:45",
            "join:/device/wifi0",
            "device:wlan0",
            "observe:/profile/candidate",
            "save:/profile/candidate",
            "commit:/checkpoint/one"
        ]
    );
}

#[test]
fn commit_requires_fresh_candidate_lan_and_never_uses_an_unrelated_fallback() {
    let bus = Bus {
        observations: [Err(NetworkError::NotReady), Ok(evidence(false))].into(),
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Commit)),
        Err(NetworkError::NotReady)
    );
    assert_eq!(
        s.execute(request(1, 3, NetworkCommand::Commit)),
        Err(NetworkError::NotReady)
    );
    assert!(!s.bus().calls.iter().any(|c| c.starts_with("save:")));
}

#[test]
fn saved_profile_followed_by_failed_checkpoint_destroy_is_not_a_commit_or_rollback_ack() {
    let bus = Bus {
        observations: [Ok(evidence(true)), Ok(evidence(true))].into(),
        fail_destroy_checkpoint: true,
        fail_rollback: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Commit)),
        Err(NetworkError::CommitUncertain)
    );
    // NM persistence and checkpoint destruction are distinct effects. This
    // fixture keeps the private saved profile despite uncertain completion.
    assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
    assert_eq!(
        &s.bus().calls[s.bus().calls.len() - 2..],
        ["save:/profile/candidate", "commit:/checkpoint/one"]
    );
    assert_eq!(
        s.execute(request(1, 3, NetworkCommand::Probe)),
        Ok(NetworkResult::Observation(evidence(true)))
    );
    assert_eq!(s.bus().calls.last().unwrap(), "observe:none");
    assert!(!s.bus().calls.iter().any(|c| c.starts_with("rollback:")));
    // A repeated commit cannot replay persistence or claim the uncertain
    // transaction completed. Read-only LAN evidence remains independent.
    assert_eq!(
        s.execute(request(1, 4, NetworkCommand::Commit)),
        Err(NetworkError::CommitUncertain)
    );
    assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
    assert_eq!(
        s.execute(request(1, 5, NetworkCommand::Rollback)),
        Err(NetworkError::CommitUncertain)
    );
    assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
    assert_eq!(
        s.execute(request(2, 1, NetworkCommand::JoinWifi(wifi()))),
        Err(NetworkError::CommitUncertain)
    );
    assert_eq!(
        s.bus()
            .calls
            .iter()
            .filter(|call| call.starts_with("join:"))
            .count(),
        1
    );
}

#[test]
fn restoration_or_checkpoint_expiry_cannot_prove_removal_of_uncertain_saved_credentials() {
    for expired_checkpoint in [false, true] {
        let bus = Bus {
            observations: [Ok(evidence(true))].into(),
            fail_destroy_checkpoint: true,
            expired_checkpoint,
            ..Bus::default()
        };
        let mut s = ManagerSession::new(selection(), bus).unwrap();
        s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
            .unwrap();
        assert_eq!(
            s.execute(request(1, 2, NetworkCommand::Commit)),
            Err(NetworkError::CommitUncertain)
        );
        assert_eq!(
            s.execute(request(1, 3, NetworkCommand::Rollback)),
            Err(NetworkError::CommitUncertain)
        );
        assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
        // The known inactive checkpoint need not permanently lock out a new
        // deliberate join, while the failed operation stays indeterminate.
        assert_eq!(
            s.execute(request(2, 1, NetworkCommand::JoinWifi(wifi()))),
            Ok(NetworkResult::Accepted)
        );
    }
}

#[test]
fn lost_save_reply_is_uncertain_even_before_checkpoint_destroy() {
    let bus = Bus {
        observations: [Ok(evidence(true))].into(),
        fail_save_after_effect: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Commit)),
        Err(NetworkError::CommitUncertain)
    );
    assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
    assert!(!s.bus().calls.iter().any(|c| c.starts_with("commit:")));
    assert_eq!(
        s.execute(request(1, 3, NetworkCommand::Rollback)),
        Err(NetworkError::CommitUncertain)
    );
    assert_eq!(s.bus().saved_profiles, ["/profile/candidate"]);
}

#[test]
fn newer_probe_is_read_only_and_superseded_commit_cannot_destroy_its_candidate() {
    let bus = Bus {
        observations: [Ok(evidence(true))].into(),
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(2, 1, NetworkCommand::Probe)),
        Ok(NetworkResult::Observation(evidence(true)))
    );
    assert_eq!(s.bus().calls.last().unwrap(), "observe:none");
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Commit)),
        Err(NetworkError::Superseded)
    );
    assert_eq!(
        s.execute(request(2, 2, NetworkCommand::Commit)),
        Err(NetworkError::Superseded)
    );
    assert!(
        !s.bus().calls.iter().any(|c| c.starts_with("rollback:")
            || c.starts_with("commit:")
            || c.starts_with("save:"))
    );
}

#[test]
fn supersession_rolls_back_before_a_new_join_and_old_receipts_cannot_mutate() {
    let mut s = ManagerSession::new(selection(), Bus::default()).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    s.execute(request(2, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Commit)),
        Err(NetworkError::Superseded)
    );
    let calls = &s.bus().calls;
    assert_eq!(calls[3], "rollback:/checkpoint/one:/device/wifi0");
    assert_eq!(calls[4], "device:wlan0");
    assert_eq!(calls[5], "checkpoint:/device/wifi0:45");
}

#[test]
fn failed_rollback_blocks_replacement_and_is_not_acknowledged() {
    let bus = Bus {
        fail_rollback: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(2, 1, NetworkCommand::JoinWifi(wifi()))),
        Err(NetworkError::RollbackFailed)
    );
    assert_eq!(
        s.bus()
            .calls
            .iter()
            .filter(|c| c.starts_with("join:"))
            .count(),
        1
    );
}

#[test]
fn failed_activation_restores_checkpoint_and_bad_device_never_means_all_devices() {
    let bus = Bus {
        fail_join: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    assert_eq!(
        s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi()))),
        Err(NetworkError::Unavailable)
    );
    assert_eq!(
        s.bus().calls.last().unwrap(),
        "rollback:/checkpoint/one:/device/wifi0"
    );
    let bus = Bus {
        bad_device: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    assert_eq!(
        s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi()))),
        Err(NetworkError::Protocol)
    );
    assert_eq!(s.bus().calls.len(), 1);
}

#[test]
fn runtime_failure_or_recovery_request_cannot_replace_a_healthy_lan_with_ap() {
    let bus = Bus {
        observations: [Ok(evidence(true))].into(),
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    assert_eq!(
        s.execute(request(1, 1, NetworkCommand::ActivateRecovery)),
        Err(NetworkError::NotReady)
    );
    assert!(!s.bus().calls.iter().any(|c| c.starts_with("activate:")));
}

#[test]
fn saved_retry_has_checkpoint_and_does_not_resave_existing_credentials() {
    let bus = Bus {
        observations: [Ok(evidence(false)), Ok(evidence(true))].into(),
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::RetrySaved))
        .unwrap();
    s.execute(request(1, 2, NetworkCommand::Commit)).unwrap();
    assert!(
        s.bus()
            .calls
            .iter()
            .any(|c| c == "checkpoint:/device/wifi0:45")
    );
    assert!(s.bus().calls.iter().any(|c| c == "observe:/profile/saved"));
    assert!(!s.bus().calls.iter().any(|c| c.starts_with("save:")));
}

#[test]
fn automatic_checkpoint_expiry_does_not_permanently_block_fresh_setup() {
    let bus = Bus {
        expired_checkpoint: true,
        ..Bus::default()
    };
    let mut s = ManagerSession::new(selection(), bus).unwrap();
    s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
        .unwrap();
    assert_eq!(
        s.execute(request(1, 2, NetworkCommand::Rollback)),
        Ok(NetworkResult::NoActiveTransaction)
    );
    assert_eq!(
        s.execute(request(2, 1, NetworkCommand::JoinWifi(wifi()))),
        Ok(NetworkResult::Accepted)
    );
}

#[test]
fn recovery_cannot_rollback_a_healthy_uncommitted_candidate() {
    for generation in [1, 2] {
        for command in [NetworkCommand::ActivateRecovery, NetworkCommand::RetrySaved] {
            let bus = Bus {
                observations: [Ok(evidence(true))].into(),
                ..Bus::default()
            };
            let mut s = ManagerSession::new(selection(), bus).unwrap();
            s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
                .unwrap();
            assert_eq!(
                s.execute(request(generation, 2, command)),
                Err(NetworkError::NotReady)
            );
            assert_eq!(s.bus().calls.last().unwrap(), "observe:none");
            assert!(
                !s.bus()
                    .calls
                    .iter()
                    .any(|c| c.starts_with("rollback:") || c.starts_with("activate:"))
            );
        }
    }
}

#[test]
fn recovery_and_retry_preserve_lan_restored_by_rollback_or_refuse_a_failed_probe() {
    for command in [NetworkCommand::ActivateRecovery, NetworkCommand::RetrySaved] {
        let bus = Bus {
            observations: [Ok(evidence(false)), Ok(evidence(true))].into(),
            ..Bus::default()
        };
        let mut s = ManagerSession::new(selection(), bus).unwrap();
        s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
            .unwrap();
        assert_eq!(
            s.execute(request(2, 1, command)),
            Err(NetworkError::NotReady)
        );
        assert_eq!(s.bus().calls.last().unwrap(), "observe:none");
        assert_eq!(
            s.bus()
                .calls
                .iter()
                .filter(|c| c.starts_with("rollback:"))
                .count(),
            1
        );
        assert!(!s.bus().calls.iter().any(|c| c.starts_with("activate:")));
    }
    for command in [NetworkCommand::ActivateRecovery, NetworkCommand::RetrySaved] {
        let bus = Bus {
            observations: [Err(NetworkError::Unavailable)].into(),
            ..Bus::default()
        };
        let mut s = ManagerSession::new(selection(), bus).unwrap();
        s.execute(request(1, 1, NetworkCommand::JoinWifi(wifi())))
            .unwrap();
        assert_eq!(
            s.execute(request(2, 1, command)),
            Err(NetworkError::Unavailable)
        );
        assert!(
            !s.bus()
                .calls
                .iter()
                .any(|c| c.starts_with("rollback:") || c.starts_with("activate:"))
        );
    }
}

#[cfg(feature = "network-manager")]
#[test]
fn disabled_real_adapter_does_not_spawn_or_connect() {
    let mut selected = selection();
    selected.enabled = false;
    let result = nm::NmAdapter::spawn(
        selected,
        std::sync::Arc::new(|| panic!("clock must not be called")),
    );
    assert!(matches!(result, Err(NetworkError::Disabled)));
}
