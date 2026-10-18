use lamp_surround::{
    atomic,
    reset::{
        Action, Failure, Kind, MAX_OPERATIONS, MAX_STATE, Outcome, Phase, RECEIPT_AGE_US,
        REQUEST_TIMEOUT_US, Receipt, Reset, State, Store,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

const NOW: u64 = 100_000_000;
fn stores() -> Vec<Store> {
    vec![
        Store::Voice,
        Store::Memory,
        Store::Schedule,
        Store::Tasks,
        Store::Settings,
        Store::Configuration,
        Store::Network,
    ]
}
fn owner(store: Store) -> String {
    format!("owner-{store:?}")
}
fn prepared() -> Outcome {
    Outcome::Prepared {
        writes_frozen: true,
        durable_barrier: true,
        voice_quiet_and_closed: true,
    }
}
fn emptied() -> Outcome {
    Outcome::Empty {
        read_back_empty: true,
        writes_frozen: true,
    }
}
fn receipt(action: &Action, at: u64, outcome: Outcome) -> Receipt {
    Receipt {
        operation_id: action.operation_id.clone(),
        coordinator_session: action.coordinator_session.clone(),
        clock_id: action.clock_id.clone(),
        request_id: action.request_id.clone(),
        owner_session: owner(action.store),
        observed_us: at,
        outcome,
    }
}

struct Root {
    _temp: tempfile::TempDir,
    path: PathBuf,
    _lock: atomic::DirectoryLock,
}
impl Root {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let lock = atomic::DirectoryLock::acquire(&path).unwrap();
        Self {
            _temp: temp,
            path,
            _lock: lock,
        }
    }
    fn record(&self) -> PathBuf {
        self.path.join("reset.json")
    }
    fn open(&self, enabled: bool, session: &str, now: u64) -> Reset {
        Reset::open(enabled, &self.record(), "boot", session, now).unwrap()
    }
    fn read(&self) -> State {
        serde_json::from_slice(&atomic::read(&self.record(), MAX_STATE).unwrap().unwrap()).unwrap()
    }
    fn write(&self, state: &State) {
        atomic::write(
            &self.record(),
            &serde_json::to_vec(state).unwrap(),
            MAX_STATE,
        )
        .unwrap();
    }
    fn obstruct_writes(&self) -> PathBuf {
        let sentinel = self.path.join("foreign");
        fs::write(&sentinel, b"keep this evidence").unwrap();
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o600)).unwrap();
        let pending = self.path.join(".reset.json.pending");
        symlink(&sentinel, &pending).unwrap();
        pending
    }
}

struct Rig {
    root: Root,
    reset: Reset,
    now: u64,
    configured: Vec<Store>,
    frozen: BTreeSet<Store>,
    nonempty: BTreeSet<Store>,
    erased: BTreeSet<String>,
    erasures: BTreeMap<Store, usize>,
    actions: Vec<Action>,
}
impl Rig {
    fn new() -> Self {
        Self::with_stores(stores())
    }
    fn with_stores(configured: Vec<Store>) -> Self {
        let root = Root::new();
        let mut reset = root.open(true, "coordinator-one", NOW);
        reset
            .begin("reset-one", configured.clone(), true, NOW)
            .unwrap();
        Self {
            root,
            reset,
            now: NOW,
            nonempty: configured.iter().copied().collect(),
            configured,
            frozen: BTreeSet::new(),
            erased: BTreeSet::new(),
            erasures: BTreeMap::new(),
            actions: vec![],
        }
    }
    fn effect(&mut self, action: &Action) -> Outcome {
        self.actions.push(action.clone());
        match action.kind {
            Kind::Prepare => {
                assert!(action.expected_owner_session.is_none());
                self.frozen.insert(action.store);
                prepared()
            }
            Kind::Empty => {
                assert!(
                    self.configured
                        .iter()
                        .all(|store| self.frozen.contains(store)),
                    "erase before every owner's durable barrier"
                );
                assert_eq!(action.expected_owner_session, Some(owner(action.store)));
                if self.erased.insert(action.request_id.clone()) {
                    self.nonempty.remove(&action.store);
                    *self.erasures.entry(action.store).or_default() += 1;
                }
                emptied()
            }
        }
    }
    fn step(&mut self) {
        let action = self.reset.next(self.now).unwrap().unwrap();
        assert_eq!(self.root.read(), *self.reset.state());
        assert_eq!(
            self.root.read().operation.unwrap().pending,
            Some(action.clone())
        );
        let revision = self.reset.state().revision;
        assert_eq!(self.reset.next(self.now).unwrap(), Some(action.clone()));
        assert_eq!(self.reset.state().revision, revision);
        let outcome = self.effect(&action);
        assert!(
            self.reset
                .observe(self.now, receipt(&action, self.now, outcome))
                .unwrap()
        );
        assert_eq!(self.root.read(), *self.reset.state());
        self.reset.state().check().unwrap();
        self.now += 1_000;
    }
    fn finish(&mut self) {
        for _ in 0..30 {
            if self.reset.state().operation.as_ref().unwrap().phase == Phase::Complete {
                return;
            }
            self.step();
        }
        panic!("reset did not complete");
    }
    fn prepare_all(&mut self) {
        for _ in 0..self.configured.len() {
            self.step();
        }
    }
}

#[test]
fn disabled_open_is_read_only_and_confirmation_is_required_before_any_intent() {
    let root = Root::new();
    let mut disabled = root.open(false, "disabled", NOW);
    assert!(!root.record().exists());
    assert!(disabled.begin("one", stores(), true, NOW).is_err());
    assert!(disabled.next(NOW).unwrap().is_none());
    assert!(!root.record().exists());
    let mut enabled = root.open(true, "enabled", NOW);
    assert!(enabled.begin("one", stores(), false, NOW).is_err());
    assert!(!root.record().exists());
    enabled.begin("one", stores(), true, NOW).unwrap();
    enabled.next(NOW).unwrap();
    let bytes = fs::read(root.record()).unwrap();
    let mut disabled = root.open(false, "enabled", NOW + 1);
    assert!(disabled.next(NOW + 1).unwrap().is_none());
    assert!(disabled.resume(NOW + 1).is_err());
    assert_eq!(fs::read(root.record()).unwrap(), bytes);
}

#[test]
fn all_owners_prepare_before_any_store_is_emptied_and_every_empty_binds_its_owner() {
    let mut rig = Rig::new();
    rig.finish();
    assert!(rig.nonempty.is_empty());
    for (index, action) in rig.actions.iter().enumerate() {
        assert_eq!(
            action.kind,
            if index < stores().len() {
                Kind::Prepare
            } else {
                Kind::Empty
            }
        );
        assert_eq!(action.store, stores()[index % stores().len()]);
    }
    assert!(
        stores()
            .iter()
            .all(|store| rig.erasures.get(store) == Some(&1))
    );
    assert_eq!(
        rig.reset.state().operation.as_ref().unwrap().completed,
        stores()
    );
    assert_eq!(
        rig.reset.state().operation.as_ref().unwrap().phase,
        Phase::Complete
    );
    assert!(rig.reset.next(rig.now).unwrap().is_none());
}

#[test]
fn voice_requires_quiet_closed_and_durable_frozen_barrier_before_any_other_owner() {
    for missing in 0..3 {
        let mut rig = Rig::new();
        let action = rig.reset.next(rig.now).unwrap().unwrap();
        let outcome = Outcome::Prepared {
            writes_frozen: missing != 0,
            durable_barrier: missing != 1,
            voice_quiet_and_closed: missing != 2,
        };
        assert!(
            rig.reset
                .observe(rig.now, receipt(&action, rig.now, outcome))
                .unwrap()
        );
        let op = rig.reset.state().operation.as_ref().unwrap();
        assert_eq!(op.phase, Phase::Incomplete);
        assert_eq!(op.failure, Some(Failure::Refused));
        assert!(op.prepared.is_empty() && op.completed.is_empty());
        assert!(rig.reset.next(rig.now).unwrap().is_none());
    }
}

#[test]
fn a_partial_erase_never_becomes_success_or_undoes_already_confirmed_stores() {
    let mut rig = Rig::new();
    rig.prepare_all();
    rig.step();
    let action = rig.reset.next(rig.now).unwrap().unwrap();
    assert_eq!(action.store, Store::Memory);
    assert!(
        rig.reset
            .observe(
                rig.now,
                receipt(
                    &action,
                    rig.now,
                    Outcome::Failed {
                        failure: Failure::Unavailable
                    }
                )
            )
            .unwrap()
    );
    let op = rig.reset.state().operation.as_ref().unwrap();
    assert_eq!(op.phase, Phase::Incomplete);
    assert_eq!(op.completed, vec![Store::Voice]);
    assert_eq!(op.failure, Some(Failure::Unavailable));
    assert!(
        rig.reset
            .next(rig.now + REQUEST_TIMEOUT_US)
            .unwrap()
            .is_none()
    );
    rig.now += REQUEST_TIMEOUT_US;
    rig.reset.resume(rig.now).unwrap();
    assert!(
        rig.reset
            .state()
            .operation
            .as_ref()
            .unwrap()
            .prepared
            .is_empty()
    );
    assert_eq!(
        rig.reset.next(rig.now).unwrap().unwrap().store,
        Store::Voice
    );
    rig.finish();
    assert!(
        stores()
            .iter()
            .all(|store| rig.erasures.get(store) == Some(&1))
    );
}

#[test]
fn empty_requires_readback_and_a_still_frozen_matching_owner() {
    for case in 0..3 {
        let mut rig = Rig::new();
        rig.prepare_all();
        let action = rig.reset.next(rig.now).unwrap().unwrap();
        let mut reply = receipt(
            &action,
            rig.now,
            Outcome::Empty {
                read_back_empty: case != 0,
                writes_frozen: case != 1,
            },
        );
        if case == 2 {
            reply.owner_session = "restarted-owner".into();
        }
        assert!(rig.reset.observe(rig.now, reply).unwrap());
        let op = rig.reset.state().operation.as_ref().unwrap();
        assert_eq!(op.phase, Phase::Incomplete);
        assert_eq!(
            op.failure,
            Some(if case == 2 {
                Failure::OwnerChanged
            } else {
                Failure::Refused
            })
        );
        assert!(op.completed.is_empty());
    }
}

#[test]
fn foreign_future_pre_intent_stale_and_duplicate_receipts_do_not_advance() {
    for case in 0..7 {
        let mut rig = Rig::new();
        let action = rig.reset.next(rig.now).unwrap().unwrap();
        let before = rig.reset.state().clone();
        let mut reply = receipt(&action, rig.now, prepared());
        let mut received = rig.now;
        match case {
            0 => reply.operation_id = "foreign".into(),
            1 => reply.request_id.push_str("-other"),
            2 => reply.coordinator_session = "old".into(),
            3 => reply.clock_id = "other-boot".into(),
            4 => reply.observed_us += 1,
            5 => reply.observed_us -= 1,
            _ => received += RECEIPT_AGE_US,
        }
        assert!(!rig.reset.observe(received, reply).unwrap());
        assert_eq!(rig.reset.state(), &before);
    }
    let mut rig = Rig::new();
    let action = rig.reset.next(rig.now).unwrap().unwrap();
    let reply = receipt(&action, rig.now, prepared());
    assert!(rig.reset.observe(rig.now, reply.clone()).unwrap());
    let after = rig.reset.state().clone();
    assert!(!rig.reset.observe(rig.now, reply).unwrap());
    assert_eq!(rig.reset.state(), &after);
}

#[test]
fn receipt_age_and_request_deadline_have_explicit_exclusive_boundaries() {
    let mut rig = Rig::new();
    let action = rig.reset.next(rig.now).unwrap().unwrap();
    assert!(
        rig.reset
            .observe(
                rig.now + RECEIPT_AGE_US - 1,
                receipt(&action, rig.now, prepared())
            )
            .unwrap()
    );
    let mut rig = Rig::new();
    let action = rig.reset.next(rig.now).unwrap().unwrap();
    assert_eq!(
        rig.reset.next(action.not_after_us - 1).unwrap(),
        Some(action.clone())
    );
    assert!(
        !rig.reset
            .observe(
                action.not_after_us,
                receipt(&action, action.not_after_us, prepared())
            )
            .unwrap()
    );
    assert!(rig.reset.next(action.not_after_us).unwrap().is_none());
    assert_eq!(
        rig.reset.state().operation.as_ref().unwrap().failure,
        Some(Failure::Timeout)
    );
    let terminal = rig.reset.state().clone();
    assert!(
        rig.reset
            .next(action.not_after_us + REQUEST_TIMEOUT_US)
            .unwrap()
            .is_none()
    );
    assert_eq!(rig.reset.state(), &terminal);
}

#[test]
fn voice_prepare_can_wait_for_quiet_but_retries_keep_the_original_deadline_and_other_actions_stay_short()
 {
    let mut rig = Rig::new();
    let voice = rig.reset.next(rig.now).unwrap().unwrap();
    assert_eq!(voice.store, Store::Voice);
    assert_eq!(voice.kind, Kind::Prepare);
    assert_eq!(
        voice.not_after_us - voice.issued_us,
        lamp_surround::reset::QUIET_TIMEOUT_US
    );
    rig.now += 45_000_001;
    assert_eq!(rig.reset.next(rig.now).unwrap(), Some(voice.clone()));
    let outcome = rig.effect(&voice);
    assert!(
        rig.reset
            .observe(rig.now, receipt(&voice, rig.now, outcome))
            .unwrap()
    );
    for _ in 0..13 {
        let action = rig.reset.next(rig.now).unwrap().unwrap();
        assert_eq!(action.not_after_us - action.issued_us, REQUEST_TIMEOUT_US);
        rig.step();
    }
    assert_eq!(
        rig.reset.state().operation.as_ref().unwrap().phase,
        Phase::Complete
    );
}

#[test]
fn power_cut_before_or_after_each_prepare_and_empty_reprepares_all_and_deduplicates_erasure() {
    for phase in 0..14 {
        for executed in [false, true] {
            let mut rig = Rig::new();
            for _ in 0..phase {
                rig.step();
            }
            let interrupted = rig.reset.next(rig.now).unwrap().unwrap();
            let outcome = if executed {
                rig.effect(&interrupted)
            } else {
                prepared()
            };
            let old_reply = receipt(&interrupted, rig.now, outcome);
            let completed = rig
                .reset
                .state()
                .operation
                .as_ref()
                .unwrap()
                .completed
                .clone();
            rig.now += 1_000;
            rig.reset = rig.root.open(true, "after-crash", rig.now);
            let op = rig.reset.state().operation.as_ref().unwrap();
            assert_eq!(op.phase, Phase::Incomplete);
            assert_eq!(op.completed, completed);
            assert!(op.pending.is_none() && op.prepared.is_empty());
            assert!(rig.reset.next(rig.now).unwrap().is_none());
            rig.reset.resume(rig.now).unwrap();
            let next = rig.reset.next(rig.now).unwrap().unwrap();
            assert_eq!(next.kind, Kind::Prepare);
            assert_eq!(next.store, Store::Voice);
            assert!(!rig.reset.observe(rig.now, old_reply).unwrap());
            rig.finish();
            assert!(
                rig.nonempty.is_empty(),
                "phase {phase}, executed {executed}"
            );
            assert!(
                stores()
                    .iter()
                    .all(|store| rig.erasures.get(store) == Some(&1))
            );
        }
    }
}

#[test]
fn completed_reset_reopens_without_repeating_erasure_or_rewriting_the_record() {
    let mut rig = Rig::new();
    rig.finish();
    let bytes = fs::read(rig.root.record()).unwrap();
    let state = rig.reset.state().clone();
    rig.reset = rig.root.open(true, "after-completion", rig.now);
    assert_eq!(rig.reset.state(), &state);
    assert!(rig.reset.next(rig.now).unwrap().is_none());
    assert_eq!(fs::read(rig.root.record()).unwrap(), bytes);
}

#[test]
fn restarting_requires_a_new_session_even_between_requests_and_a_new_boot_invalidates_barriers() {
    for with_pending in [false, true] {
        let mut rig = Rig::new();
        if with_pending {
            rig.reset.next(rig.now).unwrap();
        } else {
            rig.step();
        }
        assert!(Reset::open(true, &rig.root.record(), "boot", "coordinator-one", rig.now).is_err());
        rig.reset =
            Reset::open(true, &rig.root.record(), "new-boot", "new-coordinator", 0).unwrap();
        let op = rig.reset.state().operation.as_ref().unwrap();
        assert_eq!(op.phase, Phase::Incomplete);
        assert_eq!(op.failure, Some(Failure::ClockChanged));
        assert!(op.prepared.is_empty());
        rig.reset.resume(0).unwrap();
        let action = rig.reset.next(0).unwrap().unwrap();
        assert_eq!(action.clock_id, "new-boot");
        assert_eq!(action.issued_us, 0);
    }
}

#[test]
fn clock_regression_disables_dispatch_and_persists_incomplete_when_possible() {
    let mut rig = Rig::new();
    rig.reset.next(rig.now).unwrap();
    assert!(rig.reset.next(rig.now - 1).is_err());
    assert_eq!(
        rig.reset.state().operation.as_ref().unwrap().failure,
        Some(Failure::ClockChanged)
    );
    assert_eq!(rig.root.read(), *rig.reset.state());
    assert!(rig.reset.next(rig.now + 1).unwrap().is_none());
    assert!(rig.reset.resume(rig.now + 1).is_err());
}

#[test]
fn identity_history_prevents_a_b_a_reuse_and_is_bounded_without_eviction() {
    let mut rig = Rig::with_stores(vec![Store::Voice]);
    rig.finish();
    rig.reset
        .begin("reset-two", vec![Store::Voice], true, rig.now)
        .unwrap();
    rig.finish();
    assert!(
        rig.reset
            .begin("reset-one", vec![Store::Voice], true, rig.now)
            .is_err()
    );
    let mut state = rig.reset.state().clone();
    state.used_operation_ids = (0..MAX_OPERATIONS - 1)
        .map(|n| format!("old-{n}"))
        .collect();
    state.used_operation_ids.push("reset-two".into());
    rig.root.write(&state);
    rig.reset = rig.root.open(true, "new-coordinator", rig.now);
    let before = rig.reset.state().clone();
    assert!(
        rig.reset
            .begin("one-too-many", vec![Store::Voice], true, rig.now)
            .is_err()
    );
    assert_eq!(rig.reset.state(), &before);
}

#[test]
fn malformed_store_order_ids_and_arbitrary_paths_are_rejected_without_a_record() {
    let root = Root::new();
    let mut reset = root.open(true, "coordinator", NOW);
    for list in [
        vec![],
        vec![Store::Memory],
        vec![Store::Voice, Store::Voice],
    ] {
        assert!(reset.begin("one", list, true, NOW).is_err());
        assert!(!root.record().exists());
    }
    for id in ["", ".", "..", "../foreign", "with spaces", "bad\n"] {
        assert!(reset.begin(id, stores(), true, NOW).is_err());
        assert!(!root.record().exists());
    }
    assert!(serde_json::from_str::<Store>("\"/etc\"").is_err());
}

#[test]
fn reset_never_walks_or_removes_another_owners_files() {
    let mut rig = Rig::new();
    let foreign = rig.root.path.join("foreign-store");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("persona"), b"belongs to its owner").unwrap();
    rig.finish();
    assert_eq!(
        fs::read(foreign.join("persona")).unwrap(),
        b"belongs to its owner"
    );
}

#[test]
fn failed_intent_write_returns_no_action_and_preserves_the_previous_journal() {
    let mut rig = Rig::new();
    let before = rig.reset.state().clone();
    let obstruction = rig.root.obstruct_writes();
    assert!(rig.reset.next(rig.now).is_err());
    assert_eq!(rig.reset.state(), &before);
    assert_eq!(rig.root.read(), before);
    assert_eq!(
        fs::read(rig.root.path.join("foreign")).unwrap(),
        b"keep this evidence"
    );
    fs::remove_file(obstruction).unwrap();
    assert!(rig.reset.next(rig.now).unwrap().is_some());
}

#[test]
fn failed_ack_write_retains_the_same_idempotent_empty_request_for_recovery() {
    let mut rig = Rig::new();
    rig.prepare_all();
    let action = rig.reset.next(rig.now).unwrap().unwrap();
    let outcome = rig.effect(&action);
    let before = rig.reset.state().clone();
    let obstruction = rig.root.obstruct_writes();
    assert!(
        rig.reset
            .observe(rig.now, receipt(&action, rig.now, outcome.clone()))
            .is_err()
    );
    assert_eq!(rig.reset.state(), &before);
    assert_eq!(rig.root.read(), before);
    assert_eq!(rig.reset.next(rig.now).unwrap(), Some(action.clone()));
    fs::remove_file(obstruction).unwrap();
    assert!(
        rig.reset
            .observe(rig.now, receipt(&action, rig.now, outcome))
            .unwrap()
    );
    rig.finish();
    assert_eq!(rig.erasures.get(&Store::Voice), Some(&1));
}

#[test]
fn failed_recovery_write_exposes_no_reusable_old_reset_object() {
    let mut rig = Rig::new();
    rig.reset.next(rig.now).unwrap();
    let before = fs::read(rig.root.record()).unwrap();
    let obstruction = rig.root.obstruct_writes();
    assert!(Reset::open(true, &rig.root.record(), "boot", "restarted", rig.now).is_err());
    assert_eq!(fs::read(rig.root.record()).unwrap(), before);
    fs::remove_file(obstruction).unwrap();
    let reopened = rig.root.open(true, "restarted", rig.now);
    assert_eq!(
        reopened.state().operation.as_ref().unwrap().phase,
        Phase::Incomplete
    );
}

#[test]
fn corrupt_oversized_or_linked_state_is_refused_without_destroying_evidence() {
    let root = Root::new();
    atomic::write(&root.record(), b"{broken", MAX_STATE).unwrap();
    assert!(Reset::open(true, &root.record(), "boot", "session", NOW).is_err());
    assert_eq!(fs::read(root.record()).unwrap(), b"{broken");
    fs::write(root.record(), vec![b' '; MAX_STATE + 1]).unwrap();
    assert!(Reset::open(true, &root.record(), "boot", "session", NOW).is_err());
    fs::remove_file(root.record()).unwrap();
    let foreign = root.path.join("foreign");
    fs::write(&foreign, b"keep").unwrap();
    symlink(&foreign, root.record()).unwrap();
    assert!(Reset::open(true, &root.record(), "boot", "session", NOW).is_err());
    assert_eq!(fs::read(foreign).unwrap(), b"keep");
}

#[test]
fn persisted_completion_preparation_phase_and_pending_intent_invariants_are_checked() {
    let mut rig = Rig::new();
    rig.prepare_all();
    rig.step();
    rig.reset.next(rig.now).unwrap();
    let good = rig.reset.state().clone();
    for case in 0..13 {
        let mut bad = good.clone();
        let op = bad.operation.as_mut().unwrap();
        match case {
            0 => op.completed = vec![Store::Memory],
            1 => op.completed.push(Store::Voice),
            2 => {
                op.prepared.remove(&Store::Voice);
            }
            3 => op.phase = Phase::Complete,
            4 => op.phase = Phase::Incomplete,
            5 => op.failure = Some(Failure::Refused),
            6 => op.pending.as_mut().unwrap().kind = Kind::Prepare,
            7 => op.pending.as_mut().unwrap().store = Store::Network,
            8 => op.pending.as_mut().unwrap().request_id = "wrong".into(),
            9 => op.pending.as_mut().unwrap().expected_owner_session = Some("different".into()),
            10 => op.pending.as_mut().unwrap().coordinator_session = "different".into(),
            11 => op.pending.as_mut().unwrap().not_after_us += 1,
            _ => bad.used_operation_ids.push("duplicate-current".into()),
        }
        assert!(bad.check().is_err(), "case {case}");
        rig.root.write(&bad);
        assert!(
            Reset::open(true, &rig.root.record(), "boot", "restarted", rig.now).is_err(),
            "case {case}"
        );
    }
    rig.root.write(&good);
}

#[test]
fn directory_lock_prevents_another_writer_and_time_overflow_creates_no_intent() {
    let root = Root::new();
    assert!(atomic::DirectoryLock::acquire(&root.path).is_err());
    let mut reset = root.open(true, "coordinator", u64::MAX);
    reset.begin("overflow", stores(), true, u64::MAX).unwrap();
    let before = reset.state().clone();
    assert!(reset.next(u64::MAX).is_err());
    assert_eq!(reset.state(), &before);
    assert!(reset.state().operation.as_ref().unwrap().pending.is_none());
}

/// #1505: the reset record as a later build writes it, with a field this build
/// doesn't know. The same record without it reopens; with it, it is refused
/// today and lamp-surround doesn't start. No fix yet, and no unit starts
/// lamp-surround on a lamp: whoever makes it read one release forward flips
/// this to a plain test (Tests).
#[test]
#[should_panic(expected = "corrupt reset record")]
fn a_record_from_a_later_build_is_read() {
    let mut rig = Rig::new();
    rig.finish();
    let state = rig.reset.state().clone();
    rig.reset = rig.root.open(true, "after-completion", rig.now);
    assert_eq!(rig.reset.state(), &state);
    let mut later: serde_json::Value = serde_json::from_slice(
        &atomic::read(&rig.root.record(), MAX_STATE)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    later["a_field_from_a_later_build"] = serde_json::json!(true);
    atomic::write(
        &rig.root.record(),
        &serde_json::to_vec(&later).unwrap(),
        MAX_STATE,
    )
    .unwrap();
    rig.reset = rig.root.open(true, "later", rig.now);
    assert_eq!(rig.reset.state(), &state);
}
