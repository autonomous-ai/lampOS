//! Pure next-deadline metadata. No test treats a horizon as a reservation.
use lamp_msgs::{
    ClockStamp, ScheduleDue, ScheduleHorizon as Horizon, ScheduleHorizonClock as HorizonClock,
    ScheduleHorizonUnknown as Why,
};
use lamp_schedule::{
    engine::{Engine, MAX_ITEMS, Persistence, RetainedStoreVisibility as Visibility},
    protocol::{Guard, Request},
    time::{ClockSample, When},
};
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, Mutex},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const WALL: i64 = 1_791_964_800_000; // 2026-10-14 08:00 UTC.
const PRIVATE: &str = "PRIVATE_REMINDER_CANARY";

#[derive(Default)]
struct Data {
    bytes: Option<Vec<u8>>,
    fail: bool,
    fail_after_commit: bool,
    writes: usize,
}
#[derive(Clone, Default)]
struct Disk(Arc<Mutex<Data>>);
impl Persistence for Disk {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut data = self.0.lock().unwrap();
        if data.fail {
            return Err(io::Error::other("injected failure"));
        }
        data.bytes = Some(bytes.into());
        data.writes += 1;
        if data.fail_after_commit {
            return Err(io::Error::other("injected uncertain commit"));
        }
        Ok(())
    }
    fn set_aside(&mut self, _: &str) -> io::Result<String> {
        self.0.lock().unwrap().bytes = None;
        Ok("private-kept-store-name".into())
    }
}
fn now(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "boot-a".into(),
        wall_trusted: true,
    }
}
fn stamp(at: &ClockSample) -> ClockStamp {
    ClockStamp {
        clock_id: at.boot_id.clone(),
        at_us: at.boot_ms.saturating_mul(1000),
        sequence: 1,
    }
}
fn open(disk: Disk, at: &ClockSample) -> Engine<Disk> {
    Engine::open(disk, "Etc/UTC", at, STORE).unwrap()
}
fn guard(engine: &Engine<Disk>) -> Guard {
    Guard {
        store_id: engine.store_id().into(),
        revision: engine.revision(),
        request_id: format!("r{}", engine.revision()),
    }
}
fn set(engine: &mut Engine<Disk>, at: &ClockSample, when: When) -> Value {
    engine
        .answer(
            Request::Set {
                guard: guard(engine),
                text: PRIVATE.into(),
                person: None,
                when,
            },
            at,
        )
        .unwrap()
}
fn due(engine: &Engine<Disk>, at: &ClockSample) -> ScheduleDue {
    let value = engine.due_with_horizon(at, stamp(at), true, Visibility::Complete);
    value.check().unwrap();
    value
}
fn next(at: &ClockSample, in_ms: u64) -> Option<Horizon> {
    Some(Horizon::Next {
        sampled_boot_ms: at.boot_ms,
        in_ms,
        clock: HorizonClock::Elapsed,
    })
}
fn next_wall(at: &ClockSample, in_ms: u64) -> Option<Horizon> {
    Some(Horizon::Next {
        sampled_boot_ms: at.boot_ms,
        in_ms,
        clock: HorizonClock::Wall {
            sampled_unix_ms: at.unix_ms,
        },
    })
}
fn calendar(time: &str) -> When {
    When::At {
        time: time.into(),
        day: None,
        zone: None,
        fold: None,
    }
}
fn unknown(reason: Why) -> Option<Horizon> {
    Some(Horizon::Unknown { reason })
}

#[test]
fn empty_is_visible_without_wall_time_but_unready_and_legacy_are_not_clear() {
    let mut at = now(1234);
    at.wall_trusted = false;
    let engine = open(Disk::default(), &at);
    assert_eq!(
        due(&engine, &at).horizon,
        Some(Horizon::Empty {
            sampled_boot_ms: 1234
        })
    );
    assert_eq!(engine.due(&at, stamp(&at), true).horizon, None);
    let unavailable = engine.due_with_horizon(&at, stamp(&at), false, Visibility::Complete);
    assert!(!unavailable.ready);
    assert_eq!(unavailable.horizon, None);
}

#[test]
fn the_earliest_effective_deadline_is_read_only_even_when_it_is_already_due() {
    let disk = Disk::default();
    let mut engine = open(disk.clone(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 600 });
    set(&mut engine, &now(0), When::After { seconds: 2 });
    let revision = engine.revision();
    let writes = disk.0.lock().unwrap().writes;
    assert_eq!(due(&engine, &now(1000)).horizon, next(&now(1000), 1000));
    let elapsed = due(&engine, &now(3000));
    assert_eq!(elapsed.horizon, next(&now(3000), 0));
    assert_eq!(
        elapsed.pending, 0,
        "projection must not perform tick's commit"
    );
    assert_eq!(engine.revision(), revision);
    assert_eq!(disk.0.lock().unwrap().writes, writes);
}

#[test]
fn elapsed_timers_ignore_wall_jumps_and_continue_without_wall_trust() {
    let mut engine = open(Disk::default(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 60 });
    for wall in [WALL - 86_400_000, WALL + 86_400_000] {
        let at = ClockSample {
            unix_ms: wall,
            wall_trusted: false,
            ..now(1000)
        };
        let projected = due(&engine, &at).horizon;
        assert_eq!(projected, next(&at, 59_000));
        assert_eq!(projected.unwrap().remaining_ms(2000, None), Some(58_000));
    }
}

#[test]
fn calendar_and_rebooted_timers_require_a_trusted_wall_clock() {
    let disk = Disk::default();
    let mut engine = open(disk.clone(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 60 });
    drop(engine);
    let mut at = now(1000);
    at.boot_id = "boot-b".into();
    at.wall_trusted = false;
    let engine = open(disk, &at);
    assert_eq!(due(&engine, &at).horizon, unknown(Why::Clock));
    at.wall_trusted = true;
    let projected = due(&engine, &at).horizon;
    assert_eq!(projected, next_wall(&at, 59_000));
    assert_eq!(projected.unwrap().remaining_ms(2000, None), None);

    let mut engine = open(Disk::default(), &now(0));
    set(
        &mut engine,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let at = ClockSample {
        wall_trusted: false,
        ..now(1000)
    };
    assert_eq!(due(&engine, &at).horizon, unknown(Why::Clock));
}

#[test]
fn a_later_wall_deadline_marks_a_mixed_store_and_cannot_hide_after_a_forward_step() {
    for elapsed_first in [false, true] {
        let sample = now(0);
        let mut engine = open(Disk::default(), &sample);
        if elapsed_first {
            set(&mut engine, &sample, When::After { seconds: 1200 });
            set(&mut engine, &sample, calendar("08:30"));
        } else {
            set(&mut engine, &sample, calendar("08:30"));
            set(&mut engine, &sample, When::After { seconds: 1200 });
        }
        let projection = due(&engine, &sample);
        assert_eq!(projection.horizon, next_wall(&sample, 1_200_000));
        let later = ClockSample {
            unix_ms: WALL + 1_201_000,
            ..now(1000)
        };
        let horizon = projection.horizon.unwrap();
        // Boot ageing alone would still advertise over fifteen minutes. The
        // wall step makes the calendar item due in less than ten minutes.
        assert!(1_200_000 - later.boot_ms > 900_000);
        assert_eq!(due(&engine, &later).horizon, next_wall(&later, 599_000));
        assert_eq!(
            horizon.remaining_ms(later.boot_ms, Some(later.unix_ms)),
            Some(0)
        );
        assert_eq!(horizon.remaining_ms(later.boot_ms, None), None);
        assert!(
            !serde_json::to_string(&projection)
                .unwrap()
                .contains(PRIVATE)
        );
    }
}

#[test]
fn a_backwards_wall_step_never_extends_the_mixed_store_lower_bound() {
    let sample = now(0);
    let mut engine = open(Disk::default(), &sample);
    set(&mut engine, &sample, When::After { seconds: 1800 });
    set(&mut engine, &sample, calendar("08:20"));
    let projection = due(&engine, &sample).horizon.unwrap();
    assert_eq!(Some(projection), next_wall(&sample, 1_200_000));
    let later = ClockSample {
        unix_ms: WALL - 599_000,
        ..now(1000)
    };
    assert_eq!(due(&engine, &later).horizon, next_wall(&later, 1_799_000));
    assert_eq!(
        projection.remaining_ms(later.boot_ms, Some(later.unix_ms)),
        Some(1_199_000),
        "a conservative sample is not the newly recomputed exact minimum"
    );
}

#[test]
fn a_snoozed_calendar_item_uses_its_same_boot_elapsed_deadline() {
    let mut engine = open(Disk::default(), &now(0));
    set(&mut engine, &now(0), calendar("08:01"));
    let at = now(60_000);
    engine.tick(&at).unwrap();
    engine
        .answer(
            Request::Snooze {
                asker: None,
                guard: guard(&engine),
                id: 1,
                delivery: 1,
                seconds: 1200,
            },
            &at,
        )
        .unwrap();
    let later = ClockSample {
        wall_trusted: false,
        ..now(61_000)
    };
    let projection = due(&engine, &later).horizon;
    assert_eq!(projection, next(&later, 1_199_000));
    assert_eq!(
        projection.unwrap().remaining_ms(62_000, None),
        Some(1_198_000)
    );
}

#[test]
fn pending_is_zero_but_a_future_calendar_item_keeps_the_wall_dependency() {
    let mut engine = open(Disk::default(), &now(0));
    set(&mut engine, &now(0), calendar("08:01"));
    set(&mut engine, &now(0), calendar("08:20"));
    let at = now(60_000);
    engine.tick(&at).unwrap();
    let projection = due(&engine, &at);
    assert_eq!(projection.pending, 1);
    assert_eq!(projection.horizon, next_wall(&at, 0));
    assert_eq!(
        projection
            .horizon
            .unwrap()
            .remaining_ms(60_100, Some(WALL + 60_100)),
        Some(0)
    );
    let untrusted = ClockSample {
        wall_trusted: false,
        ..now(60_100)
    };
    assert_eq!(due(&engine, &untrusted).horizon, unknown(Why::Clock));
    engine
        .answer(
            Request::Cancel {
                asker: None,
                guard: guard(&engine),
                id: 2,
            },
            &now(60_100),
        )
        .unwrap();
    // Pending work needs no clock to remain due, even when originally set by
    // wall time. The removed future calendar item alone needed that clock.
    let only_pending = due(&engine, &untrusted).horizon;
    assert_eq!(only_pending, next(&untrusted, 0));
    assert_eq!(only_pending.unwrap().remaining_ms(60_200, None), Some(0));
}

#[test]
fn a_wall_sample_loses_eligibility_without_trust_and_saturates_after_a_step() {
    let sample = now(0);
    let mut engine = open(Disk::default(), &sample);
    set(&mut engine, &sample, calendar("08:01"));
    let projection = due(&engine, &sample).horizon.unwrap();
    assert_eq!(Some(projection), next_wall(&sample, 60_000));
    assert_eq!(projection.remaining_ms(1000, None), None);
    assert_eq!(projection.remaining_ms(1000, Some(WALL + 120_000)), Some(0));
    let untrusted = ClockSample {
        wall_trusted: false,
        ..now(1000)
    };
    assert_eq!(due(&engine, &untrusted).horizon, unknown(Why::Clock));
}

#[test]
fn pending_remains_zero_after_a_wall_jump_but_other_unknown_work_is_not_hidden() {
    let mut engine = open(Disk::default(), &now(0));
    let created = set(
        &mut engine,
        &now(0),
        When::At {
            time: "00:01".into(),
            day: None,
            zone: None,
            fold: None,
        },
    );
    let due_at = created["due_unix_ms"].as_i64().unwrap();
    let mut at = ClockSample {
        unix_ms: due_at,
        ..now(1000)
    };
    engine.tick(&at).unwrap();
    at.boot_ms += 1;
    at.unix_ms = WALL - 86_400_000;
    engine.tick(&at).unwrap();
    assert_eq!(due(&engine, &at).pending, 1);
    assert_eq!(due(&engine, &at).horizon, next(&at, 0));
    at.wall_trusted = false;
    assert_eq!(due(&engine, &at).horizon, next(&at, 0));
    at.wall_trusted = true;
    set(
        &mut engine,
        &at,
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    at.wall_trusted = false;
    assert_eq!(due(&engine, &at).horizon, unknown(Why::Clock));
}

#[test]
fn snooze_cancel_and_new_work_change_the_sample_without_reserving_the_gap() {
    let mut engine = open(Disk::default(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 1 });
    let at = now(1000);
    engine.tick(&at).unwrap();
    engine
        .answer(
            Request::Snooze {
                asker: None,
                guard: guard(&engine),
                id: 1,
                delivery: 1,
                seconds: 600,
            },
            &at,
        )
        .unwrap();
    assert_eq!(due(&engine, &at).horizon, next(&at, 600_000));
    engine
        .answer(
            Request::Cancel {
                asker: None,
                guard: guard(&engine),
                id: 1,
            },
            &at,
        )
        .unwrap();
    assert_eq!(
        due(&engine, &at).horizon,
        Some(Horizon::Empty {
            sampled_boot_ms: 1000
        })
    );
    let previous = due(&engine, &at).revision;
    set(&mut engine, &at, When::After { seconds: 1 });
    let current = due(&engine, &at);
    assert!(current.revision > previous);
    assert_eq!(current.horizon, next(&at, 1000));
}

#[test]
fn acknowledging_a_repeat_projects_the_committed_next_occurrence() {
    let mut engine = open(Disk::default(), &now(0));
    let created = set(
        &mut engine,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let at = ClockSample {
        unix_ms: created["due_unix_ms"].as_i64().unwrap(),
        ..now(1000)
    };
    engine.tick(&at).unwrap();
    assert_eq!(due(&engine, &at).horizon, next(&at, 0));
    engine
        .answer(
            Request::Ack {
                guard: guard(&engine),
                id: 1,
                delivery: 1,
            },
            &at,
        )
        .unwrap();
    assert_eq!(due(&engine, &at).horizon, next_wall(&at, 86_400_000));
}

#[test]
fn unread_and_retained_stores_are_unknown_even_when_readable_items_are_empty() {
    let disk = Disk::default();
    let mut engine = open(disk.clone(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 60 });
    drop(engine);
    let mut state: Value =
        serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    state["items"][0]["deadline"]["repeat"] =
        json!({"kind":"future_private_rule","private":PRIVATE});
    disk.0.lock().unwrap().bytes = Some(serde_json::to_vec(&state).unwrap());
    let engine = open(disk, &now(1000));
    assert_eq!(engine.unread(), 1);
    assert_eq!(due(&engine, &now(1000)).horizon, unknown(Why::Unread));
    let projected = serde_json::to_string(&due(&engine, &now(1000))).unwrap();
    assert!(!projected.contains(PRIVATE));
    assert!(!projected.contains("future_private_rule"));

    let disk = Disk::default();
    disk.0.lock().unwrap().bytes = Some(br#"{"schema":99}"#.to_vec());
    let engine = open(disk, &now(0));
    assert!(engine.set_aside().is_some());
    assert_eq!(due(&engine, &now(0)).horizon, unknown(Why::RetainedStore));
    let engine = open(Disk::default(), &now(0));
    for (visibility, why) in [
        (Visibility::Retained, Why::RetainedStore),
        (Visibility::Unknown, Why::Unavailable),
    ] {
        assert_eq!(
            engine
                .due_with_horizon(&now(0), stamp(&now(0)), true, visibility)
                .horizon,
            unknown(why)
        );
    }
}

#[test]
fn uncertain_commits_withdraw_the_horizon_including_an_empty_old_state() {
    for after_commit in [false, true] {
        let disk = Disk::default();
        let mut engine = open(disk.clone(), &now(0));
        {
            let mut data = disk.0.lock().unwrap();
            data.fail = !after_commit;
            data.fail_after_commit = after_commit;
        }
        let result = engine.answer(
            Request::Set {
                guard: guard(&engine),
                text: PRIVATE.into(),
                person: None,
                when: When::After { seconds: 1 },
            },
            &now(0),
        );
        assert!(result.is_err());
        let report = due(&engine, &now(0));
        assert!(!report.ready);
        assert_eq!(report.oldest, None);
        assert_eq!(report.horizon, None);
    }
}

#[test]
fn invalid_foreign_or_backwards_samples_never_offer_a_known_horizon() {
    let engine = open(Disk::default(), &now(1000));
    for at in [
        now(999),
        ClockSample {
            boot_id: "boot-other".into(),
            ..now(1000)
        },
        ClockSample {
            unix_ms: -1,
            ..now(1000)
        },
    ] {
        assert_eq!(due(&engine, &at).horizon, unknown(Why::Unavailable));
    }
    let mut wrong = stamp(&now(1000));
    wrong.clock_id = "boot-other".into();
    assert_eq!(
        engine
            .due_with_horizon(&now(1000), wrong, true, Visibility::Complete)
            .horizon,
        unknown(Why::Unavailable)
    );
}

#[test]
fn a_later_publication_stamp_does_not_rejuvenate_the_precommit_sample() {
    let mut engine = open(Disk::default(), &now(0));
    set(&mut engine, &now(0), When::After { seconds: 60 });
    // A durable tick may block after taking this sample. The publication's
    // MONOTONIC stamp is later, and may have a different suspend offset.
    let sample = now(10_000);
    engine.tick(&sample).unwrap();
    let published =
        engine.due_with_horizon(&sample, stamp(&now(40_000)), true, Visibility::Complete);
    assert_eq!(published.horizon, next(&sample, 50_000));
    assert_eq!(published.stamp.at_us, 40_000_000);
}

#[test]
fn every_item_fits_the_bound_and_no_private_content_enters_the_horizon() {
    let disk = Disk::default();
    let mut engine = open(disk.clone(), &now(0));
    for index in 0..MAX_ITEMS {
        set(
            &mut engine,
            &now(0),
            When::After {
                seconds: 600 + index as u64,
            },
        );
    }
    let writes = disk.0.lock().unwrap().writes;
    let report = due(&engine, &now(1000));
    let value = serde_json::to_value(report.horizon).unwrap();
    assert_eq!(
        value,
        json!({"state":"next","sampled_boot_ms":1000,"in_ms":599_000,"clock":{"basis":"elapsed"}})
    );
    assert!(!serde_json::to_string(&report).unwrap().contains(PRIVATE));
    assert_eq!(disk.0.lock().unwrap().writes, writes);
}

/// Run explicitly in release mode. This measures the full pure metadata
/// projection for 128 stored reminders against the legacy due calculation.
/// The owner computes this on each 50 ms idle tick and after requests, although
/// unchanged bus publications occur once a second. It excludes persistence,
/// bus, clock sampling and hardware; no timing assert.
#[test]
#[ignore = "manual host projection latency measurement"]
fn horizon_projection_host_measurement() {
    use std::{hint::black_box, time::Instant};
    for label in ["elapsed timers", "windowed calendar repeats"] {
        let mut engine = open(Disk::default(), &now(0));
        engine.allow_every(true);
        for index in 0..MAX_ITEMS {
            let when = if label == "elapsed timers" {
                When::After {
                    seconds: 600 + index as u64,
                }
            } else {
                When::Every {
                    minutes: Some(15),
                    hours: None,
                    from: Some("09:00".into()),
                    to: Some("17:00".into()),
                    zone: Some("America/Los_Angeles".into()),
                }
            };
            set(&mut engine, &now(0), when);
        }
        let sample = now(1000);
        let mut legacy = Vec::with_capacity(2000);
        let mut projected = Vec::with_capacity(2000);
        let mut added = Vec::with_capacity(2000);
        for iteration in 0..2000 {
            // Alternate the order to avoid always warming the same implementation.
            for horizon_first in [iteration % 2 == 0, iteration % 2 != 0] {
                let began = Instant::now();
                if horizon_first {
                    black_box(engine.due_with_horizon(
                        black_box(&sample),
                        stamp(&sample),
                        true,
                        Visibility::Complete,
                    ));
                    projected.push(began.elapsed().as_nanos());
                } else {
                    black_box(engine.due(black_box(&sample), stamp(&sample), true));
                    legacy.push(began.elapsed().as_nanos());
                }
            }
            added.push(projected[iteration] as i128 - legacy[iteration] as i128);
        }
        legacy.sort_unstable();
        projected.sort_unstable();
        added.sort_unstable();
        eprintln!("128 {label}, 2000 host sample pairs, no disk/bus/clock/hardware:");
        eprintln!(
            "legacy due: median={}ns p95={}ns maximum={}ns",
            legacy[1000], legacy[1900], legacy[1999]
        );
        eprintln!(
            "due_with_horizon: median={}ns p95={}ns maximum={}ns",
            projected[1000], projected[1900], projected[1999]
        );
        eprintln!(
            "paired added projection: median={}ns p95={}ns maximum={}ns",
            added[1000], added[1900], added[1999]
        );
    }
}
