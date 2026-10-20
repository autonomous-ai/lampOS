use crate::support;
use lamp_body::{
    control::Clock,
    runtime::{
        channel::Channels,
        wire::{Command, Control, Event, Motion},
        worker::Link,
    },
};
use lamp_turn_owner::Permission;
use std::io;
use support::*;

#[test]
fn startup_idle_then_real_ipc_inspection_and_finite_segment_reaches_checked_readback() {
    let mut r = Rig::new();
    r.authority();
    for _ in 0..8 {
        assert!(!r.step().unwrap())
    }
    assert!(r.wire.borrow().packets.is_empty());
    r.inspect();
    assert_eq!(r.wire.borrow().packets.len(), 5);
    assert!(r.wire.borrow().goals.is_empty());
    let c = r.segment();
    r.send(&c);
    let start = r.clock.now().as_micros();
    let m = r.first_goal();
    let first = r
        .events
        .iter()
        .find_map(|e| {
            if let Event::HostAccepted { at_us, .. } = e {
                Some(*at_us)
            } else {
                None
            }
        })
        .unwrap();
    assert!(first - start < 20_000);
    for _ in 0..70 {
        r.step().unwrap();
        if r.events
            .iter()
            .any(|e| matches!(e, Event::ReadbackReached { .. }))
        {
            break;
        }
    }
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e,Event::ReadbackReached{motion,..} if *motion==m))
    );
    assert!(r.wire.borrow().goals.len() > 1);
    r.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert_eq!(r.wire.borrow().discards, 1);
}
#[test]
fn cancellation_between_data_receive_and_bus_tick_never_writes_stale_segment() {
    struct Across {
        child: Channels,
        parent: Channels,
        close: Option<lamp_turn_owner::Snapshot>,
    }
    impl Link for Across {
        fn control(
            &mut self,
            now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
        ) -> io::Result<Option<Control>> {
            self.child.control.receive_with(now)
        }
        fn command(
            &mut self,
            now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
        ) -> io::Result<Option<Command>> {
            let c = self.child.data.receive_with(&mut *now)?;
            if let Some(snapshot) = self.close.take() {
                self.parent
                    .control
                    .send(&Control::Authority { snapshot }, now()?)?;
            }
            Ok(c)
        }
        fn publish(&mut self, e: &Event, n: lamp_turn_owner::MonoTime) -> io::Result<()> {
            self.child.control.send(e, n)
        }
    }
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    r.clock.advance(1);
    r.controller
        .set_microphone_permission(r.clock.now(), Permission::Denied)
        .unwrap();
    let snapshot = r.controller.snapshot(r.clock.now()).unwrap();
    let mut link = Across {
        child: r.child,
        parent: r.parent,
        close: Some(snapshot),
    };
    assert!(r.worker.as_mut().unwrap().poll(&mut link).is_err());
    assert!(r.wire.borrow().goals.is_empty());
    assert!(r.worker.take().unwrap().finish(&mut link).is_err());
    assert_eq!(r.wire.borrow().discards, 1);
}
#[test]
fn stale_cancel_and_old_snapshot_preserve_successor_but_matching_cancel_is_terminal() {
    let mut r = Rig::new();
    r.inspect();
    let old = r.controller.snapshot(r.clock.now()).unwrap();
    let c = r.segment();
    r.send(&c);
    let m = r.first_goal();
    let mut stale = m;
    stale.sequence += 1;
    r.parent
        .control
        .send(&Control::Cancel { motion: stale }, r.clock.now())
        .unwrap();
    r.parent
        .control
        .send(&Control::Authority { snapshot: old }, r.clock.now())
        .unwrap();
    assert!(!r.step().unwrap());
    assert_eq!(r.wire.borrow().discards, 0);
    r.parent
        .control
        .send(&Control::Cancel { motion: m }, r.clock.now())
        .unwrap();
    let count = r.wire.borrow().goals.len();
    assert!(r.step().unwrap());
    assert_eq!(r.wire.borrow().goals.len(), count);
    r.finish().unwrap();
    assert_eq!(r.wire.borrow().discards, 1);
}
#[test]
fn partial_position_cancel_discards_once_and_never_sends_tail() {
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    r.wire.borrow_mut().write_max = 2;
    for _ in 0..30 {
        r.step().unwrap();
        if !r.wire.borrow().partial.is_empty() {
            break;
        }
    }
    let m = r
        .events
        .iter()
        .find_map(|e| {
            if let Event::Started { motion, .. } = e {
                Some(*motion)
            } else {
                None
            }
        })
        .unwrap();
    assert!(!r.wire.borrow().partial.is_empty());
    r.parent
        .control
        .send(&Control::Cancel { motion: m }, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert!(r.wire.borrow().goals.is_empty());
    assert_eq!(r.wire.borrow().discards, 1);
    assert!(r.events.iter().any(|e|matches!(e,Event::Inhibited{report,..} if report.partial_request_bytes==2&&report.position_request)));
}
#[test]
fn partial_bus_reads_and_writes_still_use_exact_core_protocol() {
    let mut r = Rig::new();
    r.wire.borrow_mut().read_max = 16;
    r.wire.borrow_mut().write_max = 5;
    r.inspect();
    let c = r.segment();
    r.send(&c);
    r.first_goal();
    r.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    r.step().unwrap();
    r.finish().unwrap();
    assert!(
        r.wire
            .borrow()
            .packets
            .iter()
            .all(|p| p[4] == 0x82 || (p[4] == 0x83 && p[5] == 42 && p[6] == 2))
    );
}
#[test]
fn busy_command_is_rejected_without_queuing_or_replacing_original_motion() {
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    let m = r.first_goal();
    let mut later = c.clone();
    if let Command::Segment { request, .. } = &mut later {
        *request += 1
    }
    r.send(&later);
    r.step().unwrap();
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e,Event::Rejected{reason,..} if reason.contains("busy")))
    );
    r.parent
        .control
        .send(&Control::Cancel { motion: m }, r.clock.now())
        .unwrap();
    r.step().unwrap();
    r.finish().unwrap();
}
#[test]
fn expired_authority_without_next_frame_and_newer_expired_privacy_both_inhibit() {
    for delayed_revocation in [false, true] {
        let mut r = Rig::new();
        r.authority();
        r.step().unwrap();
        if delayed_revocation {
            r.controller
                .set_camera_permission(r.clock.now(), Permission::Denied)
                .unwrap();
            let s = r.controller.snapshot(r.clock.now()).unwrap();
            r.clock.advance(250_001);
            r.parent
                .control
                .send(&Control::Authority { snapshot: s }, r.clock.now())
                .unwrap();
        } else {
            r.clock.advance(250_001)
        }
        assert!(r.step().is_err());
        assert!(r.finish().is_err());
        assert_eq!(r.wire.borrow().discards, 1);
        assert!(r.wire.borrow().goals.is_empty());
    }
}
#[test]
fn malformed_priority_control_inhibits_even_when_valid_segment_waits() {
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    r.parent
        .control
        .send(
            &serde_json::json!({"kind":"stop","unexpected":true}),
            r.clock.now(),
        )
        .unwrap();
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    assert!(r.wire.borrow().goals.is_empty());
}
#[test]
fn unread_ipc_request_expires_without_renewal() {
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    r.clock.advance(100_001);
    r.authority();
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    assert!(r.wire.borrow().goals.is_empty());
}
#[test]
fn foreign_placement_and_explicit_unqualified_profiles_cannot_start() {
    for bad_profile in [false, true] {
        let mut r = Rig::new();
        if bad_profile {
            r.wire.borrow_mut().profile_speed = 0
        }
        r.inspect();
        let mut c = r.segment();
        if !bad_profile && let Command::Segment { placement, .. } = &mut c {
            placement.source_boot = b(4)
        }
        r.send(&c);
        let result = r.step();
        if bad_profile {
            assert!(result.is_err());
            assert!(r.finish().is_err());
        } else {
            assert!(!result.unwrap());
            r.parent
                .control
                .send(&Control::Stop {}, r.clock.now())
                .unwrap();
            r.step().unwrap();
            r.finish().unwrap();
        }
        assert!(r.wire.borrow().goals.is_empty());
    }
}
#[test]
fn bus_loss_bad_reply_and_slow_host_write_fault_without_retry() {
    for mode in 0..3 {
        let mut r = Rig::new();
        r.inspect();
        let c = r.segment();
        r.send(&c);
        match mode {
            0 => r.wire.borrow_mut().disconnected = true,
            1 => r.wire.borrow_mut().bad_checksum = true,
            _ => r.wire.borrow_mut().write_delay_us = 5_001,
        }
        let mut failed = false;
        for _ in 0..80 {
            if r.step().is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
        let count = r.wire.borrow().packets.len();
        assert!(r.finish().is_err());
        assert_eq!(r.wire.borrow().packets.len(), count);
        assert_eq!(r.wire.borrow().discards, 1);
    }
}
#[test]
fn feedback_backpressure_fails_closed_and_cleanup_error_remains_failure() {
    let mut r = Rig::new();
    r.authority();
    r.step().unwrap();
    let event = Event::Progress {
        at_us: r.clock.now().as_micros(),
    };
    let mut full = false;
    for _ in 0..2000 {
        match r.child.control.send(&event, r.clock.now()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                full = true;
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(full);
    r.clock.advance(10_000);
    r.wire.borrow_mut().discard_error = true;
    assert!(r.worker.as_mut().unwrap().poll(&mut r.child).is_err());
    r.drain();
    assert!(r.finish().is_err());
    assert_eq!(r.wire.borrow().discards, 1);
}
#[test]
fn ready_is_stamped_after_the_controls_read_before_it() {
    // Controls queued before the first step are read in it, the clock moving
    // on each one. Ready reports the authority installed among them, so it is
    // stamped after them: one stamped at the step's start reads old to the
    // Supervisor, which refuses a regressed receipt and times out a stale one.
    let mut r = Rig::new();
    let owner = r
        .controller
        .admit(r.clock.now(), lamp_turn_owner::AdmittedInput::NewTurn)
        .unwrap();
    let motion = Motion {
        worker: b(2),
        owner,
        sequence: 1,
    };
    r.parent
        .control
        .send(&Control::Cancel { motion }, r.clock.now())
        .unwrap();
    r.authority();
    r.clock.1.set(true);
    assert!(!r.step().unwrap());
    let at = |ready: bool| {
        r.events.iter().find_map(|e| match e {
            Event::Ready { at_us } if ready => Some(*at_us),
            Event::CancelRejected { at_us, .. } if !ready => Some(*at_us),
            _ => None,
        })
    };
    assert!(at(true).unwrap() >= at(false).unwrap(), "{:?}", r.events);
}
#[test]
fn startup_without_authority_never_touches_bus() {
    let mut r = Rig::new();
    r.clock.advance(250_000);
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    assert!(r.wire.borrow().packets.is_empty());
}

#[test]
fn data_held_behind_urgent_control_retains_original_age_and_cannot_renew() {
    struct Flood {
        child: Channels,
        parent: Channels,
        snapshot: lamp_turn_owner::Snapshot,
        once: bool,
    }
    impl Link for Flood {
        fn control(
            &mut self,
            now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
        ) -> io::Result<Option<Control>> {
            self.child.control.receive_with(now)
        }
        fn command(
            &mut self,
            now: &mut dyn FnMut() -> io::Result<lamp_turn_owner::MonoTime>,
        ) -> io::Result<Option<Command>> {
            let c = self.child.data.receive_with(&mut *now)?;
            if self.once {
                self.once = false;
                let n = now()?;
                for _ in 0..15 {
                    self.parent.control.send(
                        &Control::Authority {
                            snapshot: self.snapshot,
                        },
                        n,
                    )?;
                }
            }
            Ok(c)
        }
        fn publish(&mut self, e: &Event, n: lamp_turn_owner::MonoTime) -> io::Result<()> {
            self.child.control.send(e, n)
        }
    }
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    let snapshot = r.controller.snapshot(r.clock.now()).unwrap();
    let mut link = Flood {
        child: r.child,
        parent: r.parent,
        snapshot,
        once: true,
    };
    assert!(!r.worker.as_mut().unwrap().poll(&mut link).unwrap());
    assert!(r.wire.borrow().goals.is_empty());
    r.clock.advance(100_001);
    let snapshot = r.controller.snapshot(r.clock.now()).unwrap();
    link.parent
        .control
        .send(&Control::Authority { snapshot }, r.clock.now())
        .unwrap();
    assert!(!r.worker.as_mut().unwrap().poll(&mut link).unwrap());
    let mut rejected = false;
    while let Some(e) = link.parent.control.receive::<Event>(r.clock.now()).unwrap() {
        if matches!(e,Event::Rejected{reason,..} if reason.contains("expired")) {
            rejected = true
        }
    }
    assert!(rejected);
    assert!(r.wire.borrow().goals.is_empty());
    link.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    r.worker.as_mut().unwrap().poll(&mut link).unwrap();
    r.worker.take().unwrap().finish(&mut link).unwrap();
}

#[test]
fn input_end_revokes_an_inflight_motion_without_waiting_for_the_next_segment() {
    let mut r = Rig::new();
    r.inspect();
    let c = r.segment();
    r.send(&c);
    let m = r.first_goal();
    let count = r.wire.borrow().goals.len();
    r.controller.input_ended(r.clock.now(), m.owner).unwrap();
    r.authority();
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    assert_eq!(r.wire.borrow().goals.len(), count);
    assert_eq!(r.wire.borrow().discards, 1);
}

#[test]
fn audit_fatal_command_emits_monotonic_receipt_times() {
    let mut r = Rig::new();
    r.wire.borrow_mut().profile_speed = 0;
    r.inspect();
    let c = r.segment();
    r.send(&c);
    // Real host time advances inside a call. Existing tests hold it still until
    // the next whole step; add one microsecond at every ordinary now() read.
    r.clock.1.set(true);
    assert!(r.step().is_err());
    assert!(r.finish().is_err());
    let serialized = serde_json::to_string_pretty(&r.events).unwrap();
    if let Ok(path) = std::env::var("LAMP_MOTOR_REVIEW_EVENT_TRACE") {
        std::fs::write(path, &serialized).unwrap();
    }
    for pair in r.events.windows(2) {
        if pair[1].at_us() < pair[0].at_us() {
            println!(
                "audit_event_regression previous={:?} next={:?}",
                pair[0], pair[1]
            );
        }
    }
    assert!(
        r.events.windows(2).all(|p| p[1].at_us() >= p[0].at_us()),
        "worker emits a timestamp regression that Supervisor::event refuses"
    );
}

fn selected_command(command: Command, mask: u8) -> Command {
    let mut value = serde_json::to_value(command).unwrap();
    value["kind"] = "selected_segment".into();
    value["selection"] = mask.into();
    serde_json::from_value(value).expect("explicit selected-joint command must be supported")
}

#[test]
fn selected_command_reaches_actual_core_and_reports_exact_emitted_ids() {
    let mut r = Rig::new();
    r.inspect();
    let command = selected_command(r.segment(), 0x10);
    r.send(&command);
    let began = r.clock.now().as_micros();
    let motion = r.first_goal();
    for _ in 0..70 {
        r.step().unwrap();
        if r.events
            .iter()
            .any(|e| matches!(e, Event::ReadbackReached { .. }))
        {
            break;
        }
    }
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e,Event::ReadbackReached{motion:m,..} if *m==motion))
    );
    let receipts: Vec<_> = r
        .events
        .iter()
        .filter_map(|e| match e {
            Event::HostAccepted { .. } => Some(serde_json::to_value(e).unwrap()),
            _ => None,
        })
        .collect();
    assert!(!receipts.is_empty());
    assert!(receipts[0]["at_us"].as_u64().unwrap() - began < 20_000);
    for receipt in receipts {
        assert_eq!(receipt["selection"], 0x10);
        assert_eq!(receipt["counts"][0], 2000);
    }
    for packet in r.wire.borrow().packets.iter().filter(|p| p[4] == 0x83) {
        assert_eq!(packet.len(), 11);
        assert_eq!(&packet[5..8], &[42, 2, 5]);
    }
    assert_eq!(
        *r.wire.borrow().goals.last().unwrap(),
        [2000, 2000, 2000, 2000, 2100]
    );
    r.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert_eq!(r.wire.borrow().discards, 1);
}

#[test]
fn selected_stop_and_privacy_before_first_tick_send_no_position_or_torque() {
    for privacy in [false, true] {
        let mut r = Rig::new();
        r.inspect();
        let command = selected_command(r.segment(), 0x18);
        r.send(&command);
        if privacy {
            r.clock.advance(1);
            r.controller
                .set_microphone_permission(r.clock.now(), Permission::Denied)
                .unwrap();
            r.authority();
            assert!(r.step().is_err());
            assert!(r.finish().is_err());
        } else {
            r.parent
                .control
                .send(&Control::Stop {}, r.clock.now())
                .unwrap();
            assert!(r.step().unwrap());
            r.finish().unwrap();
        }
        assert!(r.wire.borrow().goals.is_empty());
        assert!(r.wire.borrow().packets.iter().all(|p| p[4] == 0x82));
        assert_eq!(r.wire.borrow().discards, 1);
    }
}

#[test]
fn selected_partial_cancel_never_completes_packet_or_publishes_acceptance() {
    let mut r = Rig::new();
    r.inspect();
    let command = selected_command(r.segment(), 0x18);
    r.send(&command);
    r.wire.borrow_mut().write_max = 2;
    for _ in 0..30 {
        r.step().unwrap();
        if !r.wire.borrow().partial.is_empty() {
            break;
        }
    }
    let motion = r
        .events
        .iter()
        .find_map(|e| match e {
            Event::Started { motion, .. } => Some(*motion),
            _ => None,
        })
        .unwrap();
    r.parent
        .control
        .send(&Control::Cancel { motion }, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
    assert!(r.wire.borrow().goals.is_empty());
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e, Event::HostAccepted { .. }))
    );
    assert!(r.events.iter().any(|e|matches!(e,Event::Inhibited{report,..} if report.partial_request_bytes==2&&report.position_request)));
}

#[test]
fn initial_and_reached_receipts_retain_actual_counts_and_original_read_times() {
    let mut r = Rig::new();
    r.inspect();
    let initial = r
        .events
        .iter()
        .find(|e| matches!(e, Event::Inspected { .. }))
        .unwrap();
    let value = serde_json::to_value(initial).unwrap();
    assert_eq!(value["readback"]["joints"][4]["counts"], 2000);
    assert!(
        value["readback"]["joints"][4]["received_at_us"]
            .as_u64()
            .unwrap()
            <= value["at_us"].as_u64().unwrap()
    );
    let c = r.segment();
    r.send(&c);
    r.first_goal();
    for _ in 0..70 {
        r.step().unwrap();
        if r.events
            .iter()
            .any(|e| matches!(e, Event::ReadbackReached { .. }))
        {
            break;
        }
    }
    let reached = r
        .events
        .iter()
        .find(|e| matches!(e, Event::ReadbackReached { .. }))
        .unwrap();
    let value = serde_json::to_value(reached).unwrap();
    assert_eq!(value["readback"]["joints"][4]["counts"], 2100);
    assert_eq!(value["readback"]["joints"][4]["moving_raw"], 0);
    assert_eq!(value["readback"]["joints"][4]["velocity_raw"], 0);
    r.parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    assert!(r.step().unwrap());
    r.finish().unwrap();
}
