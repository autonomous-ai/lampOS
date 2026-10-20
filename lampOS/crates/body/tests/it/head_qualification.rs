use crate::support;
use lamp_body::{
    control::Clock,
    runtime::{
        qualification::{HeadPlan, HeadQualification, OriginalEvidence, Phase},
        wire::{Control, Event, Motion, Observation},
    },
};
use lamp_turn_owner::{AdmittedInput, OutputKind, Permission};
use support::*;

fn plan() -> HeadPlan {
    HeadPlan {
        unit: "inert".into(),
        qualification_record: [7; 32],
        expected_initial_counts: [2000; 5],
        minimum_count: 1990,
        maximum_count: 2020,
        outbound_count: 2010,
        segment_duration_us: 80_000,
        maximum_duration_us: 2_000_000,
        readback_age_us: 200_000,
    }
}
fn setup() -> (Rig, HeadQualification, lamp_turn_owner::TurnOwner, u64) {
    let mut r = Rig::new();
    let owner = r
        .controller
        .admit(r.clock.now(), AdmittedInput::NewTurn)
        .unwrap();
    let mut q =
        HeadQualification::new(plan(), calibration(), b(1), b(2), b(3), r.clock.now()).unwrap();
    q.authority(r.clock.now(), r.controller.snapshot(r.clock.now()).unwrap())
        .unwrap();
    r.authority();
    (r, q, owner, 0)
}
fn evidence(
    r: &mut Rig,
    q: &mut HeadQualification,
    owner: lamp_turn_owner::TurnOwner,
    seq: &mut u64,
) -> OriginalEvidence {
    let now = r.clock.now();
    let camera = r.controller.camera_grant(now).unwrap();
    let p = r
        .controller
        .plan_output(now, owner, OutputKind::Motion, Some(camera))
        .unwrap();
    let permit = r.controller.issue_output(now, p, 240_000).unwrap();
    let snapshot = r.controller.snapshot(now).unwrap();
    q.authority(now, snapshot).unwrap();
    r.authority();
    *seq += 1;
    OriginalEvidence {
        permit,
        placement: Observation {
            source_boot: b(3),
            frame_sequence: *seq,
            acquired_at_us: now.as_micros(),
            valid_until_us: now.as_micros() + 500_000,
            camera,
            qualification_record: [7; 32],
        },
    }
}
fn until(r: &mut Rig, q: &mut HeadQualification, phase: Phase) {
    for _ in 0..100 {
        r.authority();
        q.authority(r.clock.now(), r.controller.snapshot(r.clock.now()).unwrap())
            .unwrap();
        let before = r.events.len();
        assert!(!r.step().unwrap());
        for event in &r.events[before..] {
            q.observe(r.clock.now(), event).unwrap();
        }
        if q.phase() == phase {
            return;
        }
    }
    panic!("phase {:?}, wanted {phase:?}", q.phase())
}
#[test]
fn actual_worker_out_and_exact_return_write_only_id5_and_keep_original_readbacks() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    let c = q.inspect(r.clock.now(), e).unwrap();
    r.send(&c);
    until(&mut r, &mut q, Phase::Initial);
    assert_eq!(q.last_readback().unwrap().counts(), [2000; 5]);
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    let c = q.segment(r.clock.now(), e).unwrap();
    r.send(&c);
    until(&mut r, &mut q, Phase::AtOutbound);
    assert_eq!(
        q.last_readback().unwrap().counts(),
        [2000, 2000, 2000, 2000, 2010]
    );
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    let c = q.inspect(r.clock.now(), e).unwrap();
    r.send(&c);
    until(&mut r, &mut q, Phase::ReturnReady);
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    let c = q.segment(r.clock.now(), e).unwrap();
    r.send(&c);
    until(&mut r, &mut q, Phase::Returned);
    assert_eq!(q.last_readback().unwrap().counts(), [2000; 5]);
    assert!(
        r.wire
            .borrow()
            .packets
            .iter()
            .filter(|p| p[4] == 0x83)
            .all(|p| p.len() == 11 && p[7] == 5)
    );
    let count = r.wire.borrow().goals.len();
    r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert_eq!(r.wire.borrow().goals.len(), count);
}
#[test]
fn privacy_during_outbound_never_requests_return_and_discards_pending_serial_work() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.inspect(r.clock.now(), e).unwrap());
    until(&mut r, &mut q, Phase::Initial);
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.segment(r.clock.now(), e).unwrap());
    r.first_goal();
    let count = r.wire.borrow().goals.len();
    r.controller
        .set_microphone_permission(r.clock.now(), Permission::Denied)
        .unwrap();
    assert!(
        q.authority(r.clock.now(), r.controller.snapshot(r.clock.now()).unwrap())
            .is_err()
    );
    r.authority();
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    assert_eq!(q.phase(), Phase::Inhibited);
    assert_eq!(r.wire.borrow().goals.len(), count);
    assert_eq!(r.wire.borrow().discards, 1);
    assert!(q.segment(r.clock.now(), e).is_err());
}
#[test]
fn malformed_or_old_readback_does_not_become_an_exact_return_receipt() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.inspect(r.clock.now(), e).unwrap());
    until(&mut r, &mut q, Phase::Initial);
    let readback = q.last_readback().unwrap();
    let mut bad = readback;
    bad.joints[4].received_at_us = r.clock.now().as_micros() + 1;
    assert!(bad.validate(r.clock.now(), 200_000).is_err());
    assert!(
        readback
            .validate(t(r.clock.now().as_micros() + 200_001), 200_000)
            .is_err()
    );
    let old = Event::Inspected {
        request: 1,
        at_us: r.clock.now().as_micros(),
        readback,
    };
    assert!(q.observe(r.clock.now(), &old).is_err());
    assert_eq!(q.phase(), Phase::Inhibited);
    r.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
}
#[test]
fn zero_profile_stays_rejected_and_plan_never_invents_envelope() {
    assert!(lamp_body::trajectory::RawProfile::new(0, 0, 0).is_err());
    let mut p = plan();
    p.minimum_count = 2010;
    assert!(p.validate(&calibration()).is_err());
    let mut p = plan();
    p.outbound_count = 2000;
    assert!(p.validate(&calibration()).is_err());
    let mut p = plan();
    p.unit = "other".into();
    assert!(p.validate(&calibration()).is_err());
}

#[test]
fn wrong_worker_receipt_stops_queued_motion_before_any_position_write() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.inspect(r.clock.now(), e).unwrap());
    until(&mut r, &mut q, Phase::Initial);
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.segment(r.clock.now(), e).unwrap());
    let forged = Event::Started {
        request: 2,
        motion: Motion {
            worker: b(9),
            owner,
            sequence: 1,
        },
        at_us: r.clock.now().as_micros(),
    };
    assert!(q.observe(r.clock.now(), &forged).is_err());
    r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert!(r.wire.borrow().goals.is_empty());
    assert_eq!(q.phase(), Phase::Inhibited);
}

#[test]
fn old_snapshot_cannot_revoke_newer_qualification_authority() {
    let (mut r, mut q, owner, mut seq) = setup();
    let old = r.controller.snapshot(r.clock.now()).unwrap();
    r.clock.advance(1);
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    q.authority(r.clock.now(), old).unwrap();
    assert!(q.inspect(r.clock.now(), e).is_ok());
    r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
}

#[test]
fn rejected_original_permit_permanently_inhibits_direct_command_construction() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.inspect(r.clock.now(), e).unwrap());
    until(&mut r, &mut q, Phase::Initial);
    let mut invalid = evidence(&mut r, &mut q, owner, &mut seq);
    let light = r
        .controller
        .plan_output(r.clock.now(), owner, OutputKind::Light, None)
        .unwrap();
    invalid.permit = r
        .controller
        .issue_output(r.clock.now(), light, 240_000)
        .unwrap();
    q.authority(r.clock.now(), r.controller.snapshot(r.clock.now()).unwrap())
        .unwrap();
    assert!(q.segment(r.clock.now(), invalid).is_err());
    let phase_after_error = q.phase();
    // Retain a genuinely fresh Motion permit; it must not revive this failed
    // finite qualification even though the upstream conversation is healthy.
    let now = r.clock.now();
    let camera = r.controller.camera_grant(now).unwrap();
    let output = r
        .controller
        .plan_output(now, owner, OutputKind::Motion, Some(camera))
        .unwrap();
    let permit = r.controller.issue_output(now, output, 240_000).unwrap();
    let _ = q.authority(now, r.controller.snapshot(now).unwrap());
    let fresh = OriginalEvidence {
        permit,
        placement: Observation {
            source_boot: b(3),
            frame_sequence: seq + 1,
            acquired_at_us: now.as_micros(),
            valid_until_us: now.as_micros() + 500_000,
            camera,
            qualification_record: [7; 32],
        },
    };
    let fresh_result = q.segment(r.clock.now(), fresh);
    r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert_eq!(phase_after_error, Phase::Inhibited);
    assert!(fresh_result.is_err());
    assert!(r.wire.borrow().goals.is_empty());
}

#[test]
fn rejected_inspection_phase_cannot_be_retried_as_new_authority() {
    let (mut r, mut q, owner, mut seq) = setup();
    let e = evidence(&mut r, &mut q, owner, &mut seq);
    r.send(&q.inspect(r.clock.now(), e).unwrap());
    until(&mut r, &mut q, Phase::Initial);
    let fresh = evidence(&mut r, &mut q, owner, &mut seq);
    assert!(q.inspect(r.clock.now(), fresh).is_err());
    assert_eq!(q.phase(), Phase::Inhibited);
    assert!(q.segment(r.clock.now(), fresh).is_err());
    r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert!(r.wire.borrow().goals.is_empty());
}

#[test]
fn a_prepared_exact_return_cannot_survive_stop_fault_cancellation_or_privacy() {
    for revocation in ["stop", "fault", "cancel", "microphone", "camera"] {
        let (mut r, mut q, owner, mut seq) = setup();
        let e = evidence(&mut r, &mut q, owner, &mut seq);
        r.send(&q.inspect(r.clock.now(), e).unwrap());
        until(&mut r, &mut q, Phase::Initial);
        let e = evidence(&mut r, &mut q, owner, &mut seq);
        r.send(&q.segment(r.clock.now(), e).unwrap());
        until(&mut r, &mut q, Phase::AtOutbound);
        let e = evidence(&mut r, &mut q, owner, &mut seq);
        r.send(&q.inspect(r.clock.now(), e).unwrap());
        until(&mut r, &mut q, Phase::ReturnReady);
        let e = evidence(&mut r, &mut q, owner, &mut seq);
        let writes = r.wire.borrow().goals.len();
        match revocation {
            "stop" => {
                q.stop();
            }
            "fault" => {
                assert!(
                    q.observe(
                        r.clock.now(),
                        &Event::Fault {
                            at_us: r.clock.now().as_micros(),
                            reason: "inert injected fault".into(),
                        }
                    )
                    .is_err()
                );
            }
            _ => {
                match revocation {
                    "cancel" => r.controller.cancel_turn(r.clock.now(), owner).unwrap(),
                    "microphone" => r
                        .controller
                        .set_microphone_permission(r.clock.now(), Permission::Denied)
                        .unwrap(),
                    "camera" => r
                        .controller
                        .set_camera_permission(r.clock.now(), Permission::Denied)
                        .unwrap(),
                    _ => unreachable!(),
                }
                assert!(
                    q.authority(r.clock.now(), r.controller.snapshot(r.clock.now()).unwrap())
                        .is_err()
                );
            }
        }
        assert_eq!(q.phase(), Phase::Inhibited, "{revocation}");
        assert!(
            q.segment(r.clock.now(), e).is_err(),
            "{revocation} permitted return"
        );
        r.parent.control.send(&q.stop(), r.clock.now()).unwrap();
        assert!(r.step().unwrap());
        r.finish().unwrap();
        assert_eq!(
            r.wire.borrow().goals.len(),
            writes,
            "{revocation} added a position write"
        );
        assert_eq!(q.phase(), Phase::Inhibited);
    }
}
