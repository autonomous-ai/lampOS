use lamp_surround::update::{
    Action, ActionKind, Capability, Coordinator, Evidence, FRESH_US, Health, Inventory,
    MAX_BLOCKED, MAX_RESTARTS, Outcome, Owner, Part, QUIET_US, Quiet, Receipt, ReleaseId, Request,
    RequestKind, State,
};

const NOW: u64 = 100_000_000;
fn release(byte: char) -> ReleaseId {
    ReleaseId(byte.to_string().repeat(64))
}
fn owner(part: &str, digest: &ReleaseId) -> Owner {
    Owner {
        session: format!("old-{part}"),
        release: digest.clone(),
    }
}
fn request() -> Request {
    Request {
        operation_id: "operation-one".into(),
        kind: RequestKind::Update,
        target: release('b'),
        expected_current: Some(release('a')),
    }
}
fn receipt(action: &Action, now: u64, evidence: Evidence) -> Receipt {
    Receipt {
        operation_id: action.operation_id.clone(),
        step_id: action.step_id,
        clock_id: action.clock_id.clone(),
        coordinator_session: action.coordinator_session.clone(),
        observed_us: now,
        guard: action.guard.clone(),
        evidence,
    }
}
fn checkpoint(machine: &Coordinator) -> State {
    let bytes = serde_json::to_vec(machine.state()).unwrap();
    let state: State = serde_json::from_slice(&bytes).unwrap();
    state.check().unwrap();
    state
}

/// Injected adapter, never a process or hardware mock hidden behind shell.
/// Every effect checks its preconditions and creates fresh correlated evidence.
struct Rig {
    machine: Coordinator,
    now: u64,
    inventory: Inventory,
    actions: Vec<ActionKind>,
    starts: u64,
    inspections: u64,
    body_rested: bool,
    voice_born: u64,
}
impl Rig {
    fn new() -> Self {
        let old = release('a');
        let mut machine =
            Coordinator::new(true, "boot", "coordinator-one", State::default()).unwrap();
        machine.begin(NOW, request()).unwrap();
        Self {
            machine,
            now: NOW,
            inventory: Inventory {
                current: Some(old.clone()),
                supervisor_session: "supervisor-one".into(),
                exclusive_token: "initial-fence".into(),
                voice: Some(owner("voice", &old)),
                body: Some(owner("body", &old)),
                eyes: Some(owner("eyes", &old)),
                absent_workers_gone: true,
                body_safe_when_absent: false,
            },
            actions: vec![],
            starts: 0,
            inspections: 0,
            body_rested: false,
            voice_born: 0,
        }
    }
    fn first_install() -> Self {
        let mut rig = Self::new();
        let mut request = request();
        request.expected_current = None;
        rig.machine = Coordinator::new(true, "boot", "coordinator-one", State::default()).unwrap();
        rig.machine.begin(rig.now, request).unwrap();
        rig.inventory.current = None;
        rig.inventory.voice = None;
        rig.inventory.body = None;
        rig.inventory.eyes = None;
        rig.inventory.body_safe_when_absent = true;
        rig
    }
    fn action(&mut self) -> Action {
        self.machine.tick(self.now).unwrap();
        let state = checkpoint(&self.machine);
        self.machine.committed(state.revision).unwrap().unwrap()
    }
    fn quiet(&self) -> Quiet {
        Quiet {
            voice_session: self.inventory.voice.as_ref().unwrap().session.clone(),
            observed_since_us: self.voice_born,
            last_heard_us: None,
            last_said_us: None,
            complete: true,
        }
    }
    fn effect(&mut self, action: &Action) -> Evidence {
        self.actions.push(action.kind.clone());
        assert!(self.now < action.deadline_us);
        if let Some(guard) = &action.guard {
            assert_eq!(guard.expected_current, self.inventory.current);
            assert_eq!(guard.supervisor_session, self.inventory.supervisor_session);
            assert_eq!(guard.exclusive_token, self.inventory.exclusive_token);
        }
        match &action.kind {
            ActionKind::Inspect { .. } => {
                self.inspections += 1;
                self.inventory.exclusive_token = format!("fence-{}", self.inspections);
                // The test adapter explicitly supplies fresh qualified resting
                // evidence for an absent Body. Other tests remove that proof.
                Evidence::Inspected {
                    inventory: self.inventory.clone(),
                }
            }
            ActionKind::ObserveQuiet => {
                self.now = self.now.max(self.voice_born + QUIET_US);
                Evidence::Quiet {
                    observation: self.quiet(),
                }
            }
            ActionKind::QuiesceVoice => Evidence::VoiceQuiesced {
                owner_session: self.inventory.voice.as_ref().unwrap().session.clone(),
                token: "voice-closed".into(),
                observation: self.quiet(),
            },
            ActionKind::RestBody => {
                self.body_rested = true;
                Evidence::BodyRested {
                    owner_session: self.inventory.body.as_ref().unwrap().session.clone(),
                    token: "body-rested".into(),
                    at_rest: true,
                    new_goals_blocked: true,
                }
            }
            ActionKind::StopVoice | ActionKind::StopBody | ActionKind::StopEyes => {
                let (part, slot) = match action.kind {
                    ActionKind::StopVoice => (Part::Voice, &mut self.inventory.voice),
                    ActionKind::StopBody => {
                        assert!(self.body_rested);
                        self.inventory.body_safe_when_absent = true;
                        (Part::Body, &mut self.inventory.body)
                    }
                    _ => (Part::Eyes, &mut self.inventory.eyes),
                };
                let stopped = slot.take().unwrap();
                assert_eq!(
                    action.guard.as_ref().unwrap().owner.as_ref(),
                    Some(&stopped)
                );
                Evidence::Stopped {
                    part,
                    owner_session: stopped.session,
                    quiesce_token: action.guard.as_ref().unwrap().quiesce_token.clone(),
                    all_workers_gone: true,
                }
            }
            ActionKind::SwitchSet { to } => {
                assert!(
                    self.inventory.voice.is_none()
                        && self.inventory.body.is_none()
                        && self.inventory.eyes.is_none()
                );
                assert!(self.inventory.body_safe_when_absent);
                self.inventory.current = to.clone();
                Evidence::Switched {
                    current: to.clone(),
                }
            }
            ActionKind::StartBody | ActionKind::StartEyes | ActionKind::StartVoice => {
                self.starts += 1;
                let (part, slot) = match action.kind {
                    ActionKind::StartBody => {
                        self.body_rested = false;
                        self.inventory.body_safe_when_absent = false;
                        (Part::Body, &mut self.inventory.body)
                    }
                    ActionKind::StartEyes => (Part::Eyes, &mut self.inventory.eyes),
                    _ => {
                        self.voice_born = self.now;
                        (Part::Voice, &mut self.inventory.voice)
                    }
                };
                assert!(slot.is_none(), "two owners of one capability");
                let owner = Owner {
                    session: format!("started-{}", self.starts),
                    release: self.inventory.current.clone().unwrap(),
                };
                *slot = Some(owner.clone());
                Evidence::Started { part, owner }
            }
            ActionKind::VerifyReady => {
                let capability = |owner: &Option<Owner>| Capability {
                    owner: owner.clone().unwrap(),
                    observed_us: self.now,
                    health: Health::Ready,
                };
                Evidence::Ready {
                    body: capability(&self.inventory.body),
                    eyes: capability(&self.inventory.eyes),
                    voice: capability(&self.inventory.voice),
                }
            }
            ActionKind::ReleaseOwnership => Evidence::Released,
        }
    }
    fn step(&mut self) {
        let action = self.action();
        let evidence = self.effect(&action);
        self.machine
            .observe(self.now, receipt(&action, self.now, evidence))
            .unwrap();
        checkpoint(&self.machine);
        self.now += 1_000;
    }
    fn until(&mut self, kind: ActionKind) {
        for _ in 0..40 {
            if self.machine.pending().unwrap().kind == kind {
                return;
            }
            self.step();
        }
        panic!("phase {kind:?} was never reached");
    }
    fn finish(&mut self) {
        for _ in 0..60 {
            if !self.machine.state().active() {
                return;
            }
            self.step();
        }
        panic!("transaction did not finish");
    }
    fn fail(&mut self) {
        let action = self.action();
        self.machine
            .observe(
                self.now,
                receipt(
                    &action,
                    self.now,
                    Evidence::Failed {
                        reason: "injected adapter failure".into(),
                    },
                ),
            )
            .unwrap();
        self.now += 1_000;
    }
}

#[test]
fn updates_are_disabled_without_an_explicit_switch() {
    let mut machine = Coordinator::new(false, "boot", "session", State::default()).unwrap();
    let before = machine.state().clone();
    assert!(machine.begin(NOW, request()).is_err());
    assert!(machine.tick(NOW).is_err());
    assert!(machine.committed(0).is_err());
    assert_eq!(machine.state(), &before);
}

#[test]
fn every_effect_requires_saved_current_intent_and_keeps_a_stable_dedup_key() {
    let mut rig = Rig::new();
    let action = rig.machine.pending().unwrap().clone();
    let evidence = Evidence::Inspected {
        inventory: rig.inventory.clone(),
    };
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence.clone()))
            .is_err()
    );
    assert!(
        rig.machine
            .committed(rig.machine.state().revision - 1)
            .is_err()
    );
    let saved = checkpoint(&rig.machine);
    assert_eq!(
        rig.machine.committed(saved.revision).unwrap(),
        Some(action.clone())
    );
    assert_eq!(
        rig.machine.committed(saved.revision).unwrap(),
        Some(action.clone())
    );
    rig.machine
        .observe(rig.now, receipt(&action, rig.now, evidence))
        .unwrap();
    assert!(rig.machine.committed(saved.revision).is_err());
    assert!(rig.machine.pending().unwrap().step_id > action.step_id);
}

#[test]
fn successful_update_stops_in_order_switches_one_set_and_verifies_every_capability() {
    let mut rig = Rig::new();
    rig.finish();
    assert_eq!(
        rig.actions,
        vec![
            ActionKind::Inspect {
                uncertain_step: None
            },
            ActionKind::ObserveQuiet,
            ActionKind::QuiesceVoice,
            ActionKind::StopVoice,
            ActionKind::RestBody,
            ActionKind::StopBody,
            ActionKind::StopEyes,
            ActionKind::SwitchSet {
                to: Some(release('b'))
            },
            ActionKind::StartBody,
            ActionKind::StartEyes,
            ActionKind::StartVoice,
            ActionKind::VerifyReady,
            ActionKind::ReleaseOwnership
        ]
    );
    assert_eq!(
        rig.machine.state().outcome,
        Outcome::Succeeded {
            release: release('b')
        }
    );
    assert!(rig.machine.pending().is_none());
}

#[test]
fn concurrent_requests_never_replace_the_original_previous_release() {
    let mut rig = Rig::new();
    let original = rig.machine.state().clone();
    let mut second = request();
    second.operation_id = "second-click".into();
    second.target = release('c');
    assert!(rig.machine.begin(rig.now, second).is_err());
    assert_eq!(rig.machine.state(), &original);
}

#[test]
fn quiet_requires_a_complete_window_for_both_heard_and_said_at_the_exact_boundary() {
    for bad in 0..4 {
        let mut rig = Rig::new();
        rig.until(ActionKind::ObserveQuiet);
        let action = rig.action();
        let mut observation = rig.quiet();
        observation.observed_since_us = rig.now - QUIET_US;
        observation.last_heard_us = Some(rig.now - QUIET_US);
        observation.last_said_us = Some(rig.now - QUIET_US);
        let mut refused = observation.clone();
        match bad {
            0 => refused.complete = false,
            1 => refused.observed_since_us += 1,
            2 => refused.last_heard_us = Some(rig.now - QUIET_US + 1),
            _ => refused.last_said_us = Some(rig.now - QUIET_US + 1),
        }
        assert!(
            rig.machine
                .observe(
                    rig.now,
                    receipt(
                        &action,
                        rig.now,
                        Evidence::Quiet {
                            observation: refused
                        }
                    )
                )
                .is_err()
        );
        assert_eq!(
            rig.machine.pending().unwrap().kind,
            ActionKind::ObserveQuiet
        );
        rig.machine
            .observe(
                rig.now,
                receipt(&action, rig.now, Evidence::Quiet { observation }),
            )
            .unwrap();
        assert_eq!(
            rig.machine.pending().unwrap().kind,
            ActionKind::QuiesceVoice
        );
    }
}

#[test]
fn speech_between_quiet_check_and_exclusive_quiesce_prevents_voice_stop() {
    let mut rig = Rig::new();
    rig.until(ActionKind::QuiesceVoice);
    let action = rig.action();
    let mut observation = rig.quiet();
    observation.last_heard_us = Some(rig.now);
    let evidence = Evidence::VoiceQuiesced {
        owner_session: observation.voice_session.clone(),
        token: "exclusive".into(),
        observation,
    };
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence))
            .is_err()
    );
    assert_eq!(
        rig.machine.pending().unwrap().kind,
        ActionKind::QuiesceVoice
    );
    assert!(!rig.actions.contains(&ActionKind::StopVoice));
}

#[test]
fn missing_or_foreign_owner_protocol_never_forges_a_rest_or_worker_exit() {
    for kind in [
        ActionKind::QuiesceVoice,
        ActionKind::StopVoice,
        ActionKind::RestBody,
        ActionKind::StopBody,
        ActionKind::StopEyes,
    ] {
        let mut rig = Rig::new();
        rig.until(kind.clone());
        let action = rig.action();
        let mut evidence = rig.effect(&action);
        match &mut evidence {
            Evidence::VoiceQuiesced { owner_session, .. } => *owner_session = "foreign".into(),
            Evidence::BodyRested { at_rest, .. } => *at_rest = false,
            Evidence::Stopped {
                all_workers_gone, ..
            } => *all_workers_gone = false,
            _ => unreachable!(),
        }
        assert!(
            rig.machine
                .observe(rig.now, receipt(&action, rig.now, evidence))
                .is_err()
        );
        assert_eq!(rig.machine.pending().unwrap().kind, kind);
    }
}

#[test]
fn body_rest_also_requires_new_goals_blocked_and_stop_requires_the_same_token() {
    let mut rig = Rig::new();
    rig.until(ActionKind::RestBody);
    let action = rig.action();
    let evidence = Evidence::BodyRested {
        owner_session: "old-body".into(),
        token: "rest".into(),
        at_rest: true,
        new_goals_blocked: false,
    };
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence))
            .is_err()
    );
    rig.step();
    let action = rig.action();
    let evidence = Evidence::Stopped {
        part: Part::Body,
        owner_session: "old-body".into(),
        quiesce_token: Some("other-rest".into()),
        all_workers_gone: true,
    };
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence))
            .is_err()
    );
    assert_eq!(rig.machine.pending().unwrap().kind, ActionKind::StopBody);
}

#[test]
fn foreign_duplicate_future_stale_and_pre_intent_receipts_do_not_advance() {
    for case in 0..9 {
        let mut rig = Rig::new();
        rig.until(ActionKind::ObserveQuiet);
        let action = rig.action();
        let before = rig.machine.state().clone();
        let mut reply = receipt(
            &action,
            rig.now,
            Evidence::Quiet {
                observation: rig.quiet(),
            },
        );
        let mut received = rig.now;
        match case {
            0 => reply.operation_id = "other".into(),
            1 => reply.step_id += 1,
            2 => reply.clock_id = "other-boot".into(),
            3 => reply.coordinator_session = "old-coordinator".into(),
            4 => reply.observed_us += 1,
            5 => received += FRESH_US,
            6 => reply.observed_us = action.issued_us - 1,
            7 => reply.guard.as_mut().unwrap().exclusive_token = "other-fence".into(),
            _ => reply.guard.as_mut().unwrap().supervisor_session = "restarted-supervisor".into(),
        }
        assert!(rig.machine.observe(received, reply).is_err(), "case {case}");
        assert_eq!(rig.machine.state(), &before);
    }
    let mut rig = Rig::new();
    let action = rig.action();
    let evidence = rig.effect(&action);
    let reply = receipt(&action, rig.now, evidence);
    rig.machine.observe(rig.now, reply.clone()).unwrap();
    let next = rig.machine.state().clone();
    assert!(rig.machine.observe(rig.now, reply).is_err());
    assert_eq!(rig.machine.state(), &next);
}

#[test]
fn capability_reports_must_be_fresh_at_receipt_and_for_each_actual_owner() {
    for case in 0..6 {
        let mut rig = Rig::new();
        rig.until(ActionKind::VerifyReady);
        let action = rig.action();
        let mut evidence = rig.effect(&action);
        let mut observed = rig.now;
        let mut received = rig.now;
        if let Evidence::Ready { body, eyes, voice } = &mut evidence {
            match case {
                0 => body.health = Health::Failed,
                1 => eyes.owner.session = "pid-only".into(),
                2 => voice.observed_us = action.issued_us - 1,
                3 => body.health = Health::Inhibited,
                4 => {
                    observed += FRESH_US - 1;
                    received += 2 * (FRESH_US - 1);
                }
                _ => voice.observed_us += 1,
            }
        }
        assert!(
            rig.machine
                .observe(received, receipt(&action, observed, evidence))
                .is_err(),
            "case {case}"
        );
        assert_eq!(rig.machine.pending().unwrap().kind, ActionKind::VerifyReady);
    }
}

#[test]
fn healthy_privacy_inhibition_is_explicit_and_never_forces_capture_on() {
    let mut rig = Rig::new();
    rig.until(ActionKind::VerifyReady);
    let action = rig.action();
    let mut evidence = rig.effect(&action);
    if let Evidence::Ready { voice, eyes, .. } = &mut evidence {
        voice.health = Health::Inhibited;
        eyes.health = Health::Inhibited;
    }
    rig.machine
        .observe(rig.now, receipt(&action, rig.now, evidence))
        .unwrap();
    assert_eq!(
        rig.machine.pending().unwrap().kind,
        ActionKind::ReleaseOwnership
    );
}

#[test]
fn late_timeout_is_one_recovery_attempt_and_failure_stops_automatic_work() {
    let mut rig = Rig::new();
    let deadline = rig.machine.pending().unwrap().deadline_us;
    rig.machine.tick(deadline - 1).unwrap();
    assert_eq!(rig.machine.state().outcome, Outcome::Updating);
    rig.machine.tick(deadline).unwrap();
    assert_eq!(rig.machine.state().outcome, Outcome::RollingBack);
    let next = rig.machine.pending().unwrap().deadline_us;
    rig.machine.tick(next).unwrap();
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::RecoveryFailed { .. }
    ));
    let stopped = rig.machine.state().clone();
    rig.machine.tick(next + 1_000_000_000).unwrap();
    assert_eq!(rig.machine.state(), &stopped);
    assert!(rig.machine.begin(next + 1_000_000_000, request()).is_err());
}

#[test]
fn failing_each_phase_preserves_previous_set_and_blocks_the_failed_candidate() {
    for failure_at in 0..13 {
        let mut rig = Rig::new();
        for _ in 0..failure_at {
            rig.step();
        }
        rig.fail();
        rig.finish();
        assert_eq!(
            rig.machine.state().outcome,
            Outcome::RolledBack {
                release: release('a')
            },
            "phase {failure_at}"
        );
        assert_eq!(rig.inventory.current, Some(release('a')));
        assert_eq!(rig.machine.state().blocked_candidates, vec![release('b')]);
        let mut again = request();
        again.operation_id = "automatic-again".into();
        assert!(rig.machine.begin(rig.now, again).is_err());
    }
}

#[test]
fn power_cut_before_or_after_every_effect_requires_fresh_inspection_and_old_receipts_are_dead() {
    for phase in 0..13 {
        for executed in [false, true] {
            let mut rig = Rig::new();
            for _ in 0..phase {
                rig.step();
            }
            let interrupted = rig.action();
            let saved = checkpoint(&rig.machine);
            let old_evidence = if executed {
                rig.effect(&interrupted)
            } else {
                Evidence::Released
            };
            let old_reply = receipt(&interrupted, rig.now, old_evidence);
            rig.now += 1_000;
            rig.machine = Coordinator::new(true, "boot", "coordinator-restarted", saved).unwrap();
            assert!(rig.machine.committed(rig.machine.state().revision).is_err());
            assert!(rig.machine.observe(rig.now, old_reply.clone()).is_err());
            rig.machine.tick(rig.now).unwrap();
            let action = rig.action();
            assert_eq!(
                action.kind,
                ActionKind::Inspect {
                    uncertain_step: Some(interrupted.step_id)
                }
            );
            assert!(action.guard.is_none());
            assert!(action.step_id > interrupted.step_id);
            assert!(rig.machine.observe(rig.now, old_reply).is_err());
            rig.finish();
            assert_eq!(
                rig.machine.state().outcome,
                Outcome::RolledBack {
                    release: release('a')
                },
                "phase {phase}, executed {executed}"
            );
        }
    }
}

#[test]
fn recovery_failure_preserves_an_explicit_incomplete_journal() {
    let mut rig = Rig::new();
    rig.until(ActionKind::StartVoice);
    rig.fail();
    rig.until(ActionKind::RestBody);
    rig.fail();
    let state = checkpoint(&rig.machine);
    assert!(matches!(state.outcome, Outcome::RecoveryFailed { .. }));
    assert_eq!(state.operation.as_ref().unwrap().target, Some(release('a')));
    assert_eq!(rig.inventory.current, Some(release('b')));
    assert!(!rig.actions.contains(&ActionKind::SwitchSet {
        to: Some(release('a'))
    }));
    assert!(rig.machine.pending().is_none());
    rig.machine.retry_recovery(rig.now).unwrap();
    rig.finish();
    assert_eq!(
        rig.machine.state().outcome,
        Outcome::RolledBack {
            release: release('a')
        }
    );
}

#[test]
fn absent_body_without_new_qualified_evidence_blocks_recovery_without_forced_stop() {
    let mut rig = Rig::new();
    rig.until(ActionKind::SwitchSet {
        to: Some(release('b')),
    });
    let saved = checkpoint(&rig.machine);
    rig.machine = Coordinator::new(true, "boot", "after-crash", saved).unwrap();
    rig.machine.tick(rig.now).unwrap();
    rig.inventory.body_safe_when_absent = false;
    let action = rig.action();
    let reply = receipt(
        &action,
        rig.now,
        Evidence::Inspected {
            inventory: rig.inventory.clone(),
        },
    );
    assert!(rig.machine.observe(rig.now, reply).is_err());
    assert!(matches!(
        rig.machine.pending().unwrap().kind,
        ActionKind::Inspect { .. }
    ));
    let deadline = rig.machine.pending().unwrap().deadline_us;
    rig.machine.tick(deadline).unwrap();
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::RecoveryFailed { .. }
    ));
    assert!(
        !rig.actions
            .iter()
            .any(|kind| matches!(kind, ActionKind::SwitchSet { .. }))
    );
}

#[test]
fn first_install_failure_removes_the_failed_set_and_does_not_claim_rollback_to_a_nonexistent_release()
 {
    let mut rig = Rig::first_install();
    rig.until(ActionKind::StartEyes);
    rig.fail();
    rig.finish();
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::FirstInstallFailed { .. }
    ));
    assert!(rig.inventory.current.is_none());
    assert!(
        rig.inventory.voice.is_none()
            && rig.inventory.body.is_none()
            && rig.inventory.eyes.is_none()
    );
    assert!(rig.actions.contains(&ActionKind::SwitchSet { to: None }));
    let mut other = request();
    other.operation_id = "new-first-install".into();
    other.expected_current = None;
    other.target = release('c');
    rig.machine.begin(rig.now, other).unwrap();
}

#[test]
fn manual_rollback_obeys_the_same_quiet_and_rest_boundaries() {
    let mut rig = Rig::new();
    rig.finish();
    let mut rollback = request();
    rollback.operation_id = "manual-rollback".into();
    rollback.kind = RequestKind::ManualRollback;
    rollback.target = release('a');
    rollback.expected_current = Some(release('b'));
    rig.machine.begin(rig.now, rollback).unwrap();
    let began = rig.actions.len();
    rig.finish();
    assert_eq!(
        rig.machine.state().outcome,
        Outcome::RolledBack {
            release: release('a')
        }
    );
    assert!(rig.actions[began..].contains(&ActionKind::ObserveQuiet));
    assert!(rig.actions[began..].contains(&ActionKind::RestBody));
}

#[test]
fn unexpected_current_release_or_mixed_owner_releases_are_not_overwritten() {
    let mut rig = Rig::new();
    let foreign = release('c');
    rig.inventory.current = Some(foreign.clone());
    rig.inventory.voice = Some(owner("voice", &foreign));
    rig.inventory.body = Some(owner("body", &foreign));
    rig.inventory.eyes = Some(owner("eyes", &foreign));
    let action = rig.action();
    let evidence = rig.effect(&action);
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence))
            .is_err()
    );
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::RecoveryFailed { .. }
    ));
    assert!(rig.machine.pending().is_none());
    let mut rig = Rig::new();
    rig.inventory.voice.as_mut().unwrap().release = foreign;
    let action = rig.action();
    let evidence = rig.effect(&action);
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, evidence))
            .is_err()
    );
    assert!(matches!(
        rig.machine.pending().unwrap().kind,
        ActionKind::Inspect { .. }
    ));
}

#[test]
fn restarts_and_operator_recovery_retries_have_a_fixed_budget() {
    let mut rig = Rig::new();
    for index in 0..MAX_RESTARTS {
        let state = checkpoint(&rig.machine);
        rig.machine = Coordinator::new(true, "boot", &format!("restart-{index}"), state).unwrap();
        rig.machine.tick(rig.now).unwrap();
        assert_eq!(rig.machine.state().outcome, Outcome::RollingBack);
    }
    let state = checkpoint(&rig.machine);
    rig.machine = Coordinator::new(true, "boot", "exhausted", state).unwrap();
    rig.machine.tick(rig.now).unwrap();
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::RecoveryFailed { .. }
    ));
    assert!(rig.machine.retry_recovery(rig.now).is_err());
}

#[test]
fn clock_reset_fails_closed_and_a_new_boot_must_reconcile_with_fresh_receipts() {
    let mut rig = Rig::new();
    assert!(rig.machine.tick(rig.now - 1).is_err());
    assert!(matches!(
        rig.machine.state().outcome,
        Outcome::RecoveryFailed { .. }
    ));
    let mut rig = Rig::new();
    rig.until(ActionKind::StartBody);
    let saved = checkpoint(&rig.machine);
    let old = rig.action();
    let mut rebooted = Coordinator::new(true, "new-boot", "new-session", saved).unwrap();
    rebooted.tick(0).unwrap();
    let next = rebooted
        .committed(rebooted.state().revision)
        .unwrap()
        .unwrap();
    assert_eq!(next.clock_id, "new-boot");
    assert_eq!(next.issued_us, 0);
    assert!(matches!(next.kind, ActionKind::Inspect { .. }));
    assert!(
        rebooted
            .observe(0, receipt(&old, 0, Evidence::Released))
            .is_err()
    );
}

#[test]
fn malformed_state_ids_oversized_ledgers_and_clock_overflow_are_rejected() {
    for digest in ["a", "../release", &"A".repeat(64), &"g".repeat(64)] {
        assert!(ReleaseId(digest.into()).check().is_err());
    }
    for id in ["", "../bad/path", "with spaces", "control\n"] {
        assert!(Coordinator::new(true, id, "session", State::default()).is_err());
    }
    let state = State {
        schema: 2,
        ..State::default()
    };
    assert!(Coordinator::new(true, "boot", "session", state).is_err());
    let state = State {
        blocked_candidates: vec![release('a'); MAX_BLOCKED + 1],
        ..State::default()
    };
    assert!(Coordinator::new(true, "boot", "session", state).is_err());
    let mut machine = Coordinator::new(true, "boot", "session", State::default()).unwrap();
    assert!(machine.begin(u64::MAX, request()).is_err());
    assert_eq!(machine.state(), &State::default());
}

#[test]
fn readiness_cannot_be_committed_after_it_ages_while_releasing_ownership() {
    let mut rig = Rig::new();
    rig.until(ActionKind::VerifyReady);
    let measured = rig.now;
    rig.step();
    let action = rig.action();
    assert_eq!(action.kind, ActionKind::ReleaseOwnership);
    assert_eq!(action.deadline_us, measured + FRESH_US);
    rig.machine.tick(action.deadline_us).unwrap();
    assert_eq!(rig.machine.state().outcome, Outcome::RollingBack);
    assert_eq!(rig.machine.state().blocked_candidates, vec![release('b')]);
}

#[test]
fn failure_at_every_recovery_phase_stays_incomplete_without_a_second_automatic_attempt() {
    for phase in 0..13 {
        let mut rig = Rig::new();
        rig.until(ActionKind::ReleaseOwnership);
        rig.fail();
        for _ in 0..phase {
            rig.step();
        }
        rig.fail();
        let stopped = checkpoint(&rig.machine);
        assert!(
            matches!(stopped.outcome, Outcome::RecoveryFailed { .. }),
            "phase {phase}"
        );
        assert_eq!(
            stopped.operation.as_ref().unwrap().target,
            Some(release('a'))
        );
        assert!(rig.machine.pending().is_none());
        rig.machine.tick(rig.now + 1_000_000_000).unwrap();
        assert_eq!(rig.machine.state(), &stopped);
    }
}

#[test]
fn a_wrong_switch_or_reused_start_session_never_advances() {
    let mut rig = Rig::new();
    rig.until(ActionKind::SwitchSet {
        to: Some(release('b')),
    });
    let action = rig.action();
    let wrong = Evidence::Switched {
        current: Some(release('a')),
    };
    assert!(
        rig.machine
            .observe(rig.now, receipt(&action, rig.now, wrong))
            .is_err()
    );
    assert_eq!(rig.machine.pending().unwrap().kind, action.kind);
    rig.step();
    let action = rig.action();
    for (part, session, digest) in [
        (Part::Body, "old-body", 'b'),
        (Part::Body, "new-body", 'a'),
        (Part::Voice, "new-voice", 'b'),
    ] {
        let wrong = Evidence::Started {
            part,
            owner: Owner {
                session: session.into(),
                release: release(digest),
            },
        };
        assert!(
            rig.machine
                .observe(rig.now, receipt(&action, rig.now, wrong))
                .is_err()
        );
        assert_eq!(rig.machine.pending().unwrap().kind, ActionKind::StartBody);
    }
}

#[test]
fn absent_workers_and_resting_are_independent_required_first_install_proofs() {
    for missing_workers in [false, true] {
        let mut rig = Rig::first_install();
        if missing_workers {
            rig.inventory.absent_workers_gone = false;
        } else {
            rig.inventory.body_safe_when_absent = false;
        }
        let action = rig.action();
        let evidence = rig.effect(&action);
        assert!(
            rig.machine
                .observe(rig.now, receipt(&action, rig.now, evidence))
                .is_err()
        );
        assert!(matches!(
            rig.machine.pending().unwrap().kind,
            ActionKind::Inspect { .. }
        ));
    }
}

#[test]
fn corrupted_persisted_direction_guards_and_outcomes_are_rejected_on_load() {
    let mut rig = Rig::new();
    rig.until(ActionKind::StopVoice);
    let good = checkpoint(&rig.machine);
    for case in 0..6 {
        let mut state = good.clone();
        match case {
            0 => state.operation.as_mut().unwrap().target = Some(release('c')),
            1 => {
                state
                    .operation
                    .as_mut()
                    .unwrap()
                    .pending
                    .as_mut()
                    .unwrap()
                    .guard = None
            }
            2 => {
                state
                    .operation
                    .as_mut()
                    .unwrap()
                    .pending
                    .as_mut()
                    .unwrap()
                    .guard
                    .as_mut()
                    .unwrap()
                    .exclusive_token = "bad\nfence".into()
            }
            3 => state.outcome = Outcome::RollingBack,
            4 => state.operation.as_mut().unwrap().next_step = 0,
            _ => {
                state
                    .operation
                    .as_mut()
                    .unwrap()
                    .pending
                    .as_mut()
                    .unwrap()
                    .deadline_us = 0
            }
        }
        assert!(
            Coordinator::new(true, "boot", "reloaded", state).is_err(),
            "case {case}"
        );
    }
    assert!(Coordinator::new(true, "boot", "coordinator-one", good).is_err());
}

#[test]
fn disabling_a_loaded_transaction_cannot_dispatch_its_old_side_effect() {
    let mut rig = Rig::new();
    rig.until(ActionKind::SwitchSet {
        to: Some(release('b')),
    });
    let state = checkpoint(&rig.machine);
    let mut disabled = Coordinator::new(false, "boot", "disabled-session", state.clone()).unwrap();
    assert!(disabled.tick(rig.now).is_err());
    assert!(disabled.committed(state.revision).is_err());
    assert_eq!(disabled.state(), &state);
}

#[test]
fn power_cuts_at_each_rollback_phase_keep_the_original_recovery_target() {
    for phase in 0..13 {
        for executed in [false, true] {
            let mut rig = Rig::new();
            rig.until(ActionKind::ReleaseOwnership);
            rig.fail();
            for _ in 0..phase {
                rig.step();
            }
            let interrupted = rig.action();
            let saved = checkpoint(&rig.machine);
            if executed {
                rig.effect(&interrupted);
            }
            rig.machine = Coordinator::new(true, "boot", "recovery-restarted", saved).unwrap();
            rig.machine.tick(rig.now).unwrap();
            assert_eq!(
                rig.machine.pending().unwrap().kind,
                ActionKind::Inspect {
                    uncertain_step: Some(interrupted.step_id)
                }
            );
            assert_eq!(
                rig.machine.state().operation.as_ref().unwrap().target,
                Some(release('a'))
            );
            rig.finish();
            assert_eq!(
                rig.machine.state().outcome,
                Outcome::RolledBack {
                    release: release('a')
                },
                "rollback phase {phase}, executed {executed}"
            );
            assert_eq!(rig.machine.state().blocked_candidates, vec![release('b')]);
        }
    }
}

#[test]
fn first_install_power_cuts_at_every_phase_recover_to_no_installed_set() {
    // A verified empty initial inventory skips stop/quiet phases, leaving
    // inspect, switch, three starts, capabilities, and ownership release.
    for phase in 0..7 {
        for executed in [false, true] {
            let mut rig = Rig::first_install();
            for _ in 0..phase {
                rig.step();
            }
            let interrupted = rig.action();
            let saved = checkpoint(&rig.machine);
            if executed {
                rig.effect(&interrupted);
            }
            rig.machine = Coordinator::new(true, "boot", "install-restarted", saved).unwrap();
            rig.machine.tick(rig.now).unwrap();
            rig.finish();
            assert!(
                matches!(
                    rig.machine.state().outcome,
                    Outcome::FirstInstallFailed { .. }
                ),
                "install phase {phase}, executed {executed}"
            );
            assert_eq!(rig.inventory.current, None);
            assert!(
                rig.inventory.body.is_none()
                    && rig.inventory.voice.is_none()
                    && rig.inventory.eyes.is_none()
            );
        }
    }
}

/// #1505: `update.json` as a later build writes it, with a field this build
/// doesn't know. The worker reads it with `from_slice::<update::State>` at
/// open, so today it is refused and lamp-surround doesn't start. No fix yet,
/// and no unit starts lamp-surround on a lamp: whoever makes it read one
/// release forward flips this to a plain test (Tests).
#[test]
#[should_panic(expected = "unknown field")]
fn state_from_a_later_build_is_read() {
    let bytes = serde_json::to_vec(&State::default()).unwrap();
    serde_json::from_slice::<State>(&bytes).unwrap();
    let mut later: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    later["a_field_from_a_later_build"] = serde_json::json!(true);
    serde_json::from_slice::<State>(&serde_json::to_vec(&later).unwrap()).unwrap();
}
