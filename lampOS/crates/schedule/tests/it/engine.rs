use lamp_msgs::{ClockStamp, ScheduleDue};
use lamp_schedule::{
    engine::{Engine, MAX_ITEMS, Persistence},
    protocol::{Asker, Guard, MAX_REPLY_BYTES, Request},
    time::{ClockSample, When},
};
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, Mutex},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const WALL: i64 = 1_791_964_800_000;

#[derive(Default)]
struct Data {
    bytes: Option<Vec<u8>>,
    fail: bool,
    after_commit: bool,
    writes: usize,
}
#[derive(Clone, Default)]
struct Disk(Arc<Mutex<Data>>);
impl Persistence for Disk {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.fail {
            state.fail = false;
            return Err(io::Error::other("injected disk failure"));
        }
        state.bytes = Some(bytes.into());
        state.writes += 1;
        if state.after_commit {
            state.after_commit = false;
            return Err(io::Error::other("injected directory sync failure"));
        }
        Ok(())
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
fn set(engine: &mut Engine<Disk>, at: &ClockSample, seconds: u64) -> Value {
    engine.tick(at).unwrap();
    engine
        .answer(
            Request::Set {
                person: None,
                guard: guard(engine),
                text: "Call Maria".into(),
                when: When::After { seconds },
            },
            at,
        )
        .unwrap()
}
fn create(engine: &mut Engine<Disk>, at: &ClockSample, when: When) -> Value {
    engine.tick(at).unwrap();
    engine
        .answer(
            Request::Set {
                person: None,
                guard: guard(engine),
                text: "Check the calendar".into(),
                when,
            },
            at,
        )
        .unwrap()
}
fn get(engine: &mut Engine<Disk>, at: &ClockSample, id: u64) -> Value {
    engine.answer(Request::Get { id }, at).unwrap()["item"].clone()
}
fn due(engine: &Engine<Disk>, at: &ClockSample) -> ScheduleDue {
    engine.due(
        at,
        ClockStamp {
            clock_id: at.boot_id.clone(),
            at_us: at.boot_ms * 1000,
            sequence: 1,
        },
        true,
    )
}
fn ack(engine: &mut Engine<Disk>, at: &ClockSample, id: u64, delivery: u64) -> Value {
    engine.tick(at).unwrap();
    engine
        .answer(
            Request::Ack {
                guard: guard(engine),
                id,
                delivery,
            },
            at,
        )
        .unwrap()
}

#[test]
fn a_new_store_is_empty_and_identified_before_any_request() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    let status = e.answer(Request::Status {}, &now(0)).unwrap();
    assert_eq!(status["items"], 0);
    assert_eq!(status["store_id"], STORE);
    assert_eq!(disk.0.lock().unwrap().writes, 1);
}
#[test]
fn create_reopen_and_new_ids_preserve_the_committed_timer() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    assert_eq!(set(&mut e, &now(0), 600)["id"], 1);
    drop(e);
    let mut e = open(disk, &now(1000));
    assert_eq!(get(&mut e, &now(1000), 1)["remaining_ms"], 599_000);
    assert_eq!(set(&mut e, &now(1000), 20)["id"], 2);
}
#[test]
fn even_a_one_millisecond_overdue_restart_is_marked_late() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    drop(e);
    let mut e = open(disk, &now(1001));
    e.tick(&now(1001)).unwrap();
    let notice = due(&e, &now(1001)).oldest.unwrap();
    assert!(notice.late);
    assert_eq!(notice.late_by_ms, Some(1));
}
#[test]
fn exact_due_time_is_pending_without_lateness() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 1);
    e.tick(&now(1000)).unwrap();
    let notice = due(&e, &now(1000)).oldest.unwrap();
    assert!(!notice.late);
    assert_eq!(notice.late_by_ms, Some(0));
}
#[test]
fn ordinary_poll_jitter_is_reported_but_does_not_make_a_spoken_late_claim() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 1);
    e.tick(&now(1050)).unwrap();
    assert!(!due(&e, &now(2000)).oldest.unwrap().late);
    assert!(due(&e, &now(2001)).oldest.unwrap().late);
}
#[test]
fn absent_voice_neither_consumes_due_nor_rewrites_the_store_on_every_tick() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    e.tick(&now(1000)).unwrap();
    let writes = disk.0.lock().unwrap().writes;
    for t in 2..102 {
        e.tick(&now(t * 1000)).unwrap();
        let notice = due(&e, &now(t * 1000)).oldest.unwrap();
        assert_eq!((notice.id, notice.delivery), (1, 1));
    }
    assert_eq!(disk.0.lock().unwrap().writes, writes);
}
#[test]
fn due_items_are_oldest_first_and_ack_does_not_consume_its_neighbours() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 3);
    set(&mut e, &now(0), 1);
    set(&mut e, &now(0), 2);
    e.tick(&now(5000)).unwrap();
    assert_eq!(due(&e, &now(5000)).pending, 3);
    for id in [2, 3, 1] {
        assert_eq!(due(&e, &now(5000)).oldest.unwrap().id, id);
        ack(&mut e, &now(5000), id, 1);
    }
    assert_eq!(e.counts(&now(5000)), (0, 0, 0));
}
#[test]
fn ack_requires_the_current_pending_delivery() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 1);
    let r = Request::Ack {
        guard: guard(&e),
        id: 1,
        delivery: 1,
    };
    assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "stale_delivery");
    e.tick(&now(1000)).unwrap();
    let r = Request::Ack {
        guard: guard(&e),
        id: 1,
        delivery: 2,
    };
    assert_eq!(e.answer(r, &now(1000)).unwrap_err().code, "stale_delivery");
    assert_eq!(e.counts(&now(1000)), (1, 1, 0));
}
#[test]
fn cancelling_before_or_after_due_removes_only_the_named_item() {
    for elapsed in [0, 2000] {
        let mut e = open(Disk::default(), &now(0));
        set(&mut e, &now(0), 1);
        set(&mut e, &now(0), 3);
        e.tick(&now(elapsed)).unwrap();
        let r = Request::Cancel {
            asker: None,
            guard: guard(&e),
            id: 1,
        };
        assert_eq!(e.answer(r, &now(elapsed)).unwrap()["cancelled"], true);
        e.tick(&now(4000)).unwrap();
        assert_eq!(due(&e, &now(4000)).oldest.unwrap().id, 2);
    }
}
#[test]
fn a_lost_create_reply_is_replayed_after_restart_without_a_second_timer() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 1 },
    };
    let original = e.answer(r.clone(), &now(0)).unwrap();
    drop(e);
    let mut e = open(disk, &now(2000));
    assert_eq!(e.answer(r, &now(2000)).unwrap(), original);
    assert_eq!(e.counts(&now(2000)), (1, 1, 0));
}
#[test]
fn changing_the_payload_of_a_retried_guard_is_a_conflict() {
    let mut e = open(Disk::default(), &now(0));
    let old = guard(&e);
    set(&mut e, &now(0), 10);
    let r = Request::Set {
        person: None,
        guard: old,
        text: "Different".into(),
        when: When::After { seconds: 10 },
    };
    assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "conflict");
    assert_eq!(e.counts(&now(0)).0, 1);
}
#[test]
fn an_old_retry_after_another_mutation_cannot_recreate_a_cancelled_timer() {
    let mut e = open(Disk::default(), &now(0));
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 10 },
    };
    e.answer(r.clone(), &now(0)).unwrap();
    let cancel = Request::Cancel {
        asker: None,
        guard: guard(&e),
        id: 1,
    };
    e.answer(cancel, &now(0)).unwrap();
    assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "conflict");
    assert_eq!(e.counts(&now(0)).0, 0);
}
#[test]
fn automatic_due_updates_preserve_the_last_mutation_receipt() {
    let mut e = open(Disk::default(), &now(0));
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 1 },
    };
    let first = e.answer(r.clone(), &now(0)).unwrap();
    e.tick(&now(2000)).unwrap();
    assert_eq!(e.answer(r, &now(2000)).unwrap(), first);
}

#[test]
fn a_lost_recurring_ack_receipt_cannot_ack_the_next_occurrence_after_restart() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    let mut at = now(1000);
    at.unix_ms = first + 2 * 86_400_000;
    e.tick(&at).unwrap();
    let request = Request::Ack {
        guard: guard(&e),
        id: 1,
        delivery: 1,
    };
    let receipt = e.answer(request.clone(), &at).unwrap();
    drop(e);
    // A day on, the next occurrence is due (the ack stepped to the one still
    // ahead, #811); the replayed request must not acknowledge it.
    at.boot_ms += 1000;
    at.unix_ms += 86_400_000;
    let mut e = open(disk, &at);
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).oldest.unwrap().delivery, 2);
    assert_eq!(e.answer(request, &at).unwrap(), receipt);
    assert_eq!(due(&e, &at).oldest.unwrap().delivery, 2);
}

#[test]
fn a_lost_snooze_receipt_does_not_extend_the_timer_after_restart() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    e.tick(&now(1000)).unwrap();
    let request = Request::Snooze {
        asker: None,
        guard: guard(&e),
        id: 1,
        delivery: 1,
        seconds: 10,
    };
    let receipt = e.answer(request.clone(), &now(1000)).unwrap();
    drop(e);
    let mut e = open(disk, &now(12000));
    e.tick(&now(12000)).unwrap();
    assert_eq!(due(&e, &now(12000)).oldest.unwrap().delivery, 2);
    assert_eq!(e.answer(request, &now(12000)).unwrap(), receipt);
    assert_eq!(
        due(&e, &now(12000)).oldest.unwrap().due_unix_ms,
        WALL + 11000
    );
}
#[test]
fn a_commit_error_before_write_stops_the_owner_and_does_not_announce_the_candidate() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    disk.0.lock().unwrap().fail = true;
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 1 },
    };
    assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "storage");
    assert!(e.failed());
    assert!(!due(&e, &now(0)).ready);
    drop(e);
    assert_eq!(open(disk, &now(0)).counts(&now(0)).0, 0);
}
#[test]
fn a_commit_then_sync_error_recovers_the_receipt_without_duplicate_creation() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    disk.0.lock().unwrap().after_commit = true;
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 1 },
    };
    assert!(e.answer(r.clone(), &now(0)).is_err());
    assert!(e.answer(Request::Status {}, &now(0)).is_err());
    drop(e);
    let mut e = open(disk, &now(0));
    assert_eq!(e.answer(r, &now(0)).unwrap()["id"], 1);
    assert_eq!(e.counts(&now(0)).0, 1);
}
#[test]
fn due_storage_failure_never_publishes_an_uncommitted_notice() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    disk.0.lock().unwrap().fail = true;
    assert!(e.tick(&now(1000)).is_err());
    assert!(due(&e, &now(1000)).oldest.is_none());
    drop(e);
    let mut e = open(disk, &now(2000));
    e.tick(&now(2000)).unwrap();
    assert!(due(&e, &now(2000)).oldest.is_some());
}
#[test]
fn pending_calendar_work_is_not_retracted_by_a_backwards_wall_clock() {
    let mut e = open(Disk::default(), &now(0));
    create(
        &mut e,
        &now(0),
        When::At {
            time: "00:01".into(),
            day: None,
            zone: None,
            fold: None,
        },
    );
    let due_at = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    let mut at = now(1000);
    at.unix_ms = due_at + 10;
    e.tick(&at).unwrap();
    at.boot_ms += 1;
    at.unix_ms = WALL - 86_400_000;
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).pending, 1);
    assert!(due(&e, &at).oldest.is_some());
}
#[test]
fn wall_clock_corrections_do_not_shorten_same_boot_elapsed_timers() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 600);
    let mut at = now(1000);
    at.unix_ms += 86_400_000;
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).pending, 0);
    at.wall_trusted = false;
    at.boot_ms = 600_000;
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).pending, 1);
}
#[test]
fn a_new_boot_waits_for_wall_trust_then_recovers_the_overdue_timer() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    drop(e);
    let mut at = now(5000);
    at.boot_id = "boot-b".into();
    at.boot_ms = 100;
    at.wall_trusted = false;
    let mut e = open(disk, &at);
    e.tick(&at).unwrap();
    assert_eq!(e.counts(&at), (1, 0, 1));
    at.wall_trusted = true;
    e.tick(&at).unwrap();
    assert!(due(&e, &at).oldest.unwrap().late);
}
/// #811: the lamp has no battery clock, so after a reboot its wall clock is a
/// day behind until chrony sets it, and the product schedule trusts it only
/// once the kernel says so (`--kernel-clock`). In that window a daily
/// reminder neither falls due nor is lost, and nothing calendar-timed can be
/// made from the wrong date; once the clock is set it falls due once, then
/// waits for tomorrow.
#[test]
fn after_a_reboot_with_the_clock_a_day_behind_nothing_calendar_timed_acts_until_it_is_set() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let due_at = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    assert_eq!(due_at, WALL + 3_600_000);
    drop(e);

    // Rebooted ten seconds after the reminder's time, the clock a day back.
    let mut at = now(0);
    at.boot_id = "boot-b".into();
    at.boot_ms = 1_000;
    at.unix_ms = due_at + 10_000 - 86_400_000;
    at.wall_trusted = false;
    let mut e = open(disk, &at);
    for step in 0..70 {
        at.boot_ms += 1_000;
        at.unix_ms += 1_000;
        e.tick(&at).unwrap();
        assert_eq!(due(&e, &at).pending, 0, "{step} s after boot");
    }
    let made = e.answer(
        Request::Set {
            person: None,
            guard: guard(&e),
            text: "Tea".into(),
            when: When::At {
                time: "10:00".into(),
                day: None,
                zone: None,
                fold: None,
            },
        },
        &at,
    );
    assert_eq!(made.unwrap_err().code, "bad_time");

    // chrony sets it: the reminder falls due once, as late as it really is.
    at.boot_ms += 1_000;
    at.unix_ms = due_at + 81_000;
    at.wall_trusted = true;
    e.tick(&at).unwrap();
    let shown = due(&e, &at);
    assert_eq!(shown.pending, 1);
    assert!(shown.oldest.unwrap().late);
}

/// #811, the first builder's 13:21: after three days off, a daily reminder
/// is delivered once, at its newest missed occurrence and marked late, then
/// goes on to the next one still ahead. Nobody hears 'late' four times.
#[test]
fn after_three_days_off_a_daily_reminder_is_delivered_once_late_then_goes_on() {
    const DAY: i64 = 86_400_000;
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    drop(e);

    // Off from before the first 09:00 until an hour after the fourth.
    let mut at = now(0);
    at.boot_id = "boot-b".into();
    at.boot_ms = 1_000;
    at.unix_ms = first + 3 * DAY + 3_600_000;
    let mut e = open(disk, &at);
    e.tick(&at).unwrap();
    let shown = due(&e, &at);
    assert_eq!(shown.pending, 1);
    let oldest = shown.oldest.unwrap();
    assert!(oldest.late);
    let item = get(&mut e, &at, 1);
    assert_eq!(
        item["due_unix_ms"].as_i64(),
        Some(first + 3 * DAY),
        "the newest one"
    );
    ack(&mut e, &at, 1, oldest.delivery);

    // Nothing more today; tomorrow's comes on time.
    at.boot_ms += 60_000;
    at.unix_ms += 60_000;
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).pending, 0);
    assert_eq!(
        get(&mut e, &at, 1)["due_unix_ms"].as_i64(),
        Some(first + 4 * DAY)
    );
    at.boot_ms += 1_000;
    at.unix_ms = first + 4 * DAY;
    e.tick(&at).unwrap();
    let shown = due(&e, &at);
    assert_eq!(shown.pending, 1);
    assert!(!shown.oldest.unwrap().late);
}

/// The third's review of #1046: the oldest pending notice is chosen by the
/// occurrence each stands for. A daily missed for three days is due at its
/// newest 09:00, so a one-off that fell due an hour before that comes first.
#[test]
fn after_time_off_the_oldest_notice_is_chosen_by_the_occurrence_shown() {
    const DAY: i64 = 86_400_000;
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    // A one-off at 08:00 three days on: an hour before the newest 09:00.
    let mut later = now(1_000);
    later.unix_ms = first + 3 * DAY - 3_600_000 - 60_000;
    // At a clock time: a one-minute timer an hour and a half late is too
    // late to ring (#5258); a reminder is not.
    let set = create(
        &mut e,
        &later,
        When::At {
            time: "08:00".into(),
            day: None,
            zone: None,
            fold: None,
        },
    );
    assert_eq!(set["id"], 2);
    drop(e);
    let mut at = now(0);
    at.boot_id = "boot-b".into();
    at.boot_ms = 1_000;
    at.unix_ms = first + 3 * DAY + 1_800_000;
    let mut e = open(disk, &at);
    e.tick(&at).unwrap();
    let shown = due(&e, &at);
    assert_eq!(shown.pending, 2);
    assert_eq!(shown.oldest.unwrap().id, 2, "the one-off fell due first");
}

#[test]
fn untrusted_wall_time_cannot_create_a_false_reboot_deadline() {
    let mut at = now(0);
    at.wall_trusted = false;
    let mut e = open(Disk::default(), &at);
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Tea".into(),
        when: When::After { seconds: 1 },
    };
    assert_eq!(e.answer(r, &at).unwrap_err().code, "bad_time");
}
#[test]
fn backward_elapsed_clock_or_boot_replacement_is_refused_within_one_owner() {
    let mut e = open(Disk::default(), &now(100));
    assert_eq!(e.tick(&now(99)).unwrap_err().code, "clock");
    let mut at = now(101);
    at.boot_id = "other".into();
    assert_eq!(e.tick(&at).unwrap_err().code, "clock");
}
/// Long downtime: the occurrences missed are not said one after another
/// (#811, the first builder's 13:21); the newest is said once, then the
/// repeat goes on from the next one still ahead.
#[test]
fn long_downtime_says_only_the_newest_missed_occurrence_once() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    drop(e);
    let mut at = now(1);
    at.unix_ms = first + 25 * 86_400_000;
    let mut e = open(disk, &at);
    e.tick(&at).unwrap();
    let n = due(&e, &at).oldest.unwrap();
    assert_eq!(n.delivery, 1);
    assert_eq!(n.due_unix_ms, first + 25 * 86_400_000);
    assert!(n.late);
    ack(&mut e, &at, 1, 1);
    e.tick(&at).unwrap();
    assert!(due(&e, &at).oldest.is_none());
    assert_eq!(e.counts(&at).0, 1);
    assert_eq!(
        get(&mut e, &at, 1)["due_unix_ms"].as_i64(),
        Some(first + 26 * 86_400_000)
    );
}
#[test]
fn snoozing_invalidates_old_delivery_but_does_not_shift_the_repeat_anchor() {
    let mut e = open(Disk::default(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    let mut at = now(1000);
    at.unix_ms = first;
    e.tick(&at).unwrap();
    let r = Request::Snooze {
        asker: None,
        guard: guard(&e),
        id: 1,
        delivery: 1,
        seconds: 600,
    };
    e.answer(r, &at).unwrap();
    assert!(due(&e, &at).oldest.is_none());
    at.boot_ms += 600_000;
    at.unix_ms += 600_000;
    e.tick(&at).unwrap();
    let stale = Request::Ack {
        guard: guard(&e),
        id: 1,
        delivery: 1,
    };
    assert_eq!(e.answer(stale, &at).unwrap_err().code, "stale_delivery");
    ack(&mut e, &at, 1, 2);
    assert_eq!(get(&mut e, &at, 1)["due_unix_ms"], first + 86_400_000);
}
#[test]
fn snooze_requires_a_due_occurrence() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 60);
    let r = Request::Snooze {
        asker: None,
        guard: guard(&e),
        id: 1,
        delivery: 1,
        seconds: 1,
    };
    assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "stale_delivery");
}
#[test]
fn capacity_refuses_new_work_without_evicting_a_due_reminder() {
    let mut e = open(Disk::default(), &now(0));
    for _ in 0..MAX_ITEMS {
        set(&mut e, &now(0), 1);
    }
    e.tick(&now(2000)).unwrap();
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: "Extra".into(),
        when: When::After { seconds: 1 },
    };
    assert_eq!(e.answer(r, &now(2000)).unwrap_err().code, "full");
    assert_eq!(e.counts(&now(2000)), (128, 128, 0));
}
#[test]
fn removing_a_timer_frees_capacity_without_reusing_its_id() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 1);
    e.tick(&now(1000)).unwrap();
    ack(&mut e, &now(1000), 1, 1);
    assert_eq!(set(&mut e, &now(1000), 1)["id"], 2);
}
#[test]
fn wrong_store_or_invalid_request_identity_cannot_mutate() {
    for bad in [
        Guard {
            store_id: "fedcba9876543210fedcba9876543210".into(),
            revision: 0,
            request_id: "x".into(),
        },
        Guard {
            store_id: STORE.into(),
            revision: 0,
            request_id: "bad request".into(),
        },
    ] {
        let mut e = open(Disk::default(), &now(0));
        let r = Request::Set {
            person: None,
            guard: bad,
            text: "Tea".into(),
            when: When::After { seconds: 1 },
        };
        assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "bad_guard");
        assert_eq!(e.counts(&now(0)).0, 0);
    }
}
#[test]
fn text_is_unicode_preserving_and_never_silently_truncated() {
    let mut e = open(Disk::default(), &now(0));
    let text = "Nhắc tôi gọi Maria";
    let r = Request::Set {
        person: None,
        guard: guard(&e),
        text: text.into(),
        when: When::After { seconds: 1 },
    };
    e.answer(r, &now(0)).unwrap();
    assert_eq!(get(&mut e, &now(0), 1)["text"], text);
    for text in [" ".into(), "a\nb".into(), "a\0b".into(), "é".repeat(151)] {
        let r = Request::Set {
            person: None,
            guard: guard(&e),
            text,
            when: When::After { seconds: 1 },
        };
        assert_eq!(e.answer(r, &now(0)).unwrap_err().code, "bad_text");
    }
}
#[test]
fn paginated_listing_visits_each_item_once_without_mutation() {
    let mut e = open(Disk::default(), &now(0));
    for _ in 0..19 {
        set(&mut e, &now(0), 60);
    }
    let before = e.revision();
    let mut after_id = 0;
    let mut ids = Vec::new();
    loop {
        let page = e
            .answer(
                Request::List {
                    after_id,
                    limit: 8,
                    pending_only: false,
                },
                &now(0),
            )
            .unwrap();
        ids.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["id"].as_u64().unwrap()),
        );
        match page["next_after_id"].as_u64() {
            Some(id) => after_id = id,
            None => break,
        }
    }
    assert_eq!(ids, (1..=19).collect::<Vec<_>>());
    assert_eq!(e.revision(), before);
}
#[test]
fn pending_filter_and_page_bounds_are_explicit() {
    let mut e = open(Disk::default(), &now(0));
    set(&mut e, &now(0), 1);
    set(&mut e, &now(0), 10);
    let page = e
        .answer(
            Request::List {
                after_id: 0,
                limit: 8,
                pending_only: true,
            },
            &now(2000),
        )
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    for limit in [0, 9, u16::MAX] {
        assert_eq!(
            e.answer(
                Request::List {
                    after_id: 0,
                    limit,
                    pending_only: false
                },
                &now(2000)
            )
            .unwrap_err()
            .code,
            "bad_request"
        );
    }
}
#[test]
fn maximum_text_and_repeat_metadata_fit_a_whole_list_reply() {
    let mut e = open(Disk::default(), &now(0));
    for _ in 0..8 {
        let mut g = guard(&e);
        g.request_id = "x".repeat(64);
        let r = Request::Set {
            person: None,
            guard: g,
            text: "\"".repeat(300),
            when: When::Weekly {
                weekday: lamp_schedule::time::Weekday::Monday,
                time: "09:00".into(),
                zone: Some("America/Argentina/ComodRivadavia".into()),
            },
        };
        e.answer(r, &now(0)).unwrap();
    }
    let before = e.revision();
    let mut after_id = 0;
    let mut ids = Vec::new();
    loop {
        let page = e
            .answer(
                Request::List {
                    after_id,
                    limit: 8,
                    pending_only: false,
                },
                &now(0),
            )
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_REPLY_BYTES);
        let items = page["items"].as_array().unwrap();
        assert!(!items.is_empty());
        for item in items {
            assert_eq!(item["text"], "\"".repeat(300));
            assert_eq!(item["request_id"], "x".repeat(64));
            ids.push(item["id"].as_u64().unwrap());
        }
        match page["next_after_id"].as_u64() {
            Some(id) => {
                assert!(id > after_id);
                after_id = id;
            }
            None => break,
        }
    }
    assert_eq!(ids, (1..=8).collect::<Vec<_>>());
    assert_eq!(e.revision(), before);
}
#[test]
fn malformed_state_or_a_backend_without_set_aside_is_not_overwritten() {
    for bytes in [
        b"{cut".to_vec(),
        serde_json::to_vec(&json!({"schema":99})).unwrap(),
    ] {
        let disk = Disk::default();
        disk.0.lock().unwrap().bytes = Some(bytes.clone());
        assert!(Engine::open(disk.clone(), "Etc/UTC", &now(0), STORE).is_err());
        assert_eq!(disk.load().unwrap(), Some(bytes));
    }
}
#[test]
fn corrupt_persisted_item_ids_and_deadlines_fail_closed() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 1);
    drop(e);
    let original: Value = serde_json::from_slice(&disk.load().unwrap().unwrap()).unwrap();
    let mut variants = Vec::new();
    let mut v = original.clone();
    v["items"][0]["id"] = 0.into();
    variants.push(v);
    let mut v = original.clone();
    v["items"][0]["deadline"]["local"] = "incoherent".into();
    variants.push(v);
    let mut v = original.clone();
    v["next_id"] = 1.into();
    variants.push(v);
    let mut v = original;
    v["items"][0]["text"] = "".into();
    variants.push(v);
    for v in variants {
        let d = Disk::default();
        d.0.lock().unwrap().bytes = Some(serde_json::to_vec(&v).unwrap());
        assert!(Engine::open(d, "Etc/UTC", &now(0), STORE).is_err());
    }
}

/// #1505: a store as a later build writes it, with a field this build doesn't
/// know at its top and in a reminder. It opens with its revision kept: a field
/// added by a later build is passed over, as the same store without it is
/// read (Tests wrote it failing; Second's forward read makes it pass).
#[test]
fn a_store_from_a_later_build_is_read() {
    let disk = Disk::default();
    let mut engine = open(disk.clone(), &now(0));
    set(&mut engine, &now(0), 600);
    let revision = engine.revision();
    drop(engine);
    assert_eq!(open(disk.clone(), &now(1)).revision(), revision);
    let mut later: Value =
        serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    later["a_field_from_a_later_build"] = json!(true);
    later["items"][0]["a_field_from_a_later_build"] = json!(true);
    disk.0.lock().unwrap().bytes = Some(serde_json::to_vec(&later).unwrap());
    assert_eq!(open(disk, &now(2)).revision(), revision);
}

/// What setup does: a new link to the zone renamed over `localtime`.
fn point(root: &std::path::Path, name: &str) {
    let file = root.join("zoneinfo").join(name);
    if !file.exists() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, jiff_tzdb::get(name).unwrap().1).unwrap();
    }
    let link = root.join("localtime.new");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&file, &link).unwrap();
    std::fs::rename(&link, root.join("localtime")).unwrap();
}

/// #820: with `--zone system`, a reminder is set in the zone setup last
/// pointed the lamp at, without a restart: a daily 7:00 set in Ho Chi Minh
/// City, then one set after a re-setup to New York, each at its own 7:00.
/// The first keeps the zone it was set in.
#[test]
fn with_the_system_zone_a_new_reminder_follows_a_re_setup_and_an_old_one_keeps_its_own() {
    let root = tempfile::tempdir().unwrap();
    point(root.path(), "Asia/Ho_Chi_Minh");
    let system = lamp_local_time::LocalTime::System(
        lamp_local_time::SystemZone::open(
            root.path().join("localtime"),
            root.path().join("timezone"),
        )
        .unwrap(),
    );
    let at = now(0);
    let mut engine = Engine::open_following(Disk::default(), system, &at, STORE).unwrap();
    let status =
        |engine: &mut Engine<Disk>| engine.answer(Request::Status {}, &at).unwrap()["zone"].clone();
    assert_eq!(status(&mut engine), "Asia/Ho_Chi_Minh");
    let daily = || When::Daily {
        time: "07:00".into(),
        zone: None,
    };
    let saigon = create(&mut engine, &at, daily());
    assert!(
        saigon["local"]
            .as_str()
            .unwrap()
            .contains("T07:00:00+07:00[Asia/Ho_Chi_Minh]"),
        "{saigon}"
    );

    point(root.path(), "America/New_York");
    assert_eq!(status(&mut engine), "America/New_York");
    let new_york = create(&mut engine, &at, daily());
    let local = new_york["local"].as_str().unwrap();
    assert!(
        local.contains("T07:00:00-0") && local.ends_with("[America/New_York]"),
        "{new_york}"
    );
    let first = get(&mut engine, &at, saigon["id"].as_u64().unwrap());
    assert!(first.to_string().contains("Asia/Ho_Chi_Minh"), "{first}");
}

/// A system zone pointed at `name` in a folder of its own.
fn system_at(root: &std::path::Path, name: &str) -> lamp_local_time::LocalTime {
    point(root, name);
    lamp_local_time::LocalTime::System(
        lamp_local_time::SystemZone::open(root.join("localtime"), root.join("timezone")).unwrap(),
    )
}

/// #1680: a zone in `zone.env` this build's time zone rules don't know (a
/// newer tzdb's) never stops the part. It falls back to the system's zone,
/// says so, and reminders still ring: one already set at the time it was set
/// for, in its own zone, and a new one with no zone at its time in the
/// system's (07:00 is 07:00 in Ho Chi Minh City).
#[test]
fn an_unknown_zone_falls_back_to_the_systems_and_reminders_still_ring() {
    let disk = Disk::default();
    let at = now(0);
    let mut paris = Engine::open(disk.clone(), "Europe/Paris", &at, STORE).unwrap();
    let daily = || When::Daily {
        time: "07:00".into(),
        zone: None,
    };
    let old = create(&mut paris, &at, daily());
    drop(paris);
    let root = tempfile::tempdir().unwrap();
    let system = system_at(root.path(), "Asia/Ho_Chi_Minh");
    let mut engine = Engine::open_with_system(
        disk,
        "Middle_Earth/Rivendell",
        || Ok(system),
        None,
        &at,
        STORE,
    )
    .expect("an unknown zone does not stop the part");
    assert_eq!(
        engine.zone_fallback(),
        Some(("Middle_Earth/Rivendell", "Asia/Ho_Chi_Minh"))
    );
    assert_eq!(
        engine.answer(Request::Status {}, &at).unwrap()["zone"],
        "Asia/Ho_Chi_Minh"
    );
    let new = create(&mut engine, &at, daily());
    assert!(
        new["local"]
            .as_str()
            .unwrap()
            .contains("T07:00:00+07:00[Asia/Ho_Chi_Minh]"),
        "{new}"
    );
    let id = old["id"].as_u64().unwrap();
    let kept = get(&mut engine, &at, id);
    assert_eq!(kept["local"], old["local"], "{kept}");
    let local = kept["local"].as_str().unwrap();
    assert!(
        local.contains("T07:00:00+0") && local.ends_with("[Europe/Paris]"),
        "{kept}"
    );
    // At its time it rings, and not a moment before.
    let due_at = old["due_unix_ms"].as_i64().unwrap();
    let before = now(u64::try_from(due_at - WALL - 1).unwrap());
    engine.tick(&before).unwrap();
    assert_eq!(get(&mut engine, &before, id)["state"], "scheduled");
    let ring = now(u64::try_from(due_at - WALL).unwrap());
    engine.tick(&ring).unwrap();
    assert_eq!(get(&mut engine, &ring, id)["state"], "pending");
}

/// #1680, the reviewer's P2 on #2066: setup points the system's zone at the
/// same name as `zone.env`'s, so a zone this build doesn't know (a newer
/// tzdb's America/Coyhaique, -03:00) has no system zone to fall back to.
/// The fallback is then the offset `zone.env` stored: "at 7 tomorrow" rings
/// at 07:00 at the lamp, not at 07:00 UTC (04:00 there).
#[test]
fn an_unknown_zone_falls_back_to_the_stored_utc_offset() {
    let at = now(0);
    let unreadable = || Err(io::Error::other("no system zone"));
    let mut engine = Engine::open_with_system(
        Disk::default(),
        "Middle_Earth/Rivendell",
        unreadable,
        Some(-180),
        &at,
        STORE,
    )
    .expect("an unknown zone does not stop the part");
    assert_eq!(
        engine.zone_fallback(),
        Some(("Middle_Earth/Rivendell", "Etc/GMT+3"))
    );
    for when in [
        When::At {
            time: "07:00".into(),
            day: Some("tomorrow".into()),
            zone: None,
            fold: None,
        },
        When::Daily {
            time: "07:00".into(),
            zone: None,
        },
    ] {
        let new = create(&mut engine, &at, when);
        assert!(
            new["local"]
                .as_str()
                .unwrap()
                .contains("T07:00:00-03:00[Etc/GMT+3]"),
            "{new}"
        );
    }
    // East of UTC too, and `--zone system` with an unreadable system zone.
    let engine =
        Engine::open_with_system(Disk::default(), "system", unreadable, Some(420), &at, STORE)
            .unwrap();
    assert_eq!(engine.zone_fallback(), Some(("system", "Etc/GMT-7")));
}

/// #1680: with no zone this build knows and no whole-hour offset stored,
/// the fallback is UTC, and a new reminder at a clock time with no zone of
/// its own is refused with a reason, since it would ring hours off; one in
/// minutes from now, or at a time in a named zone, is set. The same for
/// `--zone system` when the system's zone names no zone this build knows,
/// which was refused at start.
#[test]
fn with_no_known_zone_or_offset_a_clock_time_is_refused_and_minutes_still_work() {
    let at = now(0);
    let unreadable = || Err(io::Error::other("no system zone"));
    for offset in [None, Some(330)] {
        let mut engine = Engine::open_with_system(
            Disk::default(),
            "Middle_Earth/Rivendell",
            unreadable,
            offset,
            &at,
            STORE,
        )
        .expect("an unknown zone does not stop the part");
        assert_eq!(
            engine.zone_fallback(),
            Some(("Middle_Earth/Rivendell", "Etc/UTC"))
        );
        for when in [
            When::At {
                time: "07:00".into(),
                day: Some("tomorrow".into()),
                zone: None,
                fold: None,
            },
            When::Daily {
                time: "07:00".into(),
                zone: None,
            },
            When::Weekly {
                weekday: lamp_schedule::time::Weekday::Monday,
                time: "07:00".into(),
                zone: None,
            },
        ] {
            engine.tick(&at).unwrap();
            let refused = engine
                .answer(
                    Request::Set {
                        person: None,
                        guard: guard(&engine),
                        text: "Call Maria".into(),
                        when,
                    },
                    &at,
                )
                .unwrap_err();
            assert_eq!(refused.code, "bad_time", "{offset:?}");
            assert!(refused.message.contains("clock time"), "{refused}");
        }
        let later = set(&mut engine, &at, 1_200);
        assert_eq!(later["due_unix_ms"], WALL + 1_200_000, "{later}");
        let named = create(
            &mut engine,
            &at,
            When::Daily {
                time: "07:00".into(),
                zone: Some("Asia/Ho_Chi_Minh".into()),
            },
        );
        assert!(
            named["local"]
                .as_str()
                .unwrap()
                .contains("T07:00:00+07:00[Asia/Ho_Chi_Minh]"),
            "{named}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("localtime"),
        jiff_tzdb::get("Asia/Ho_Chi_Minh").unwrap().1,
    )
    .unwrap();
    let nameless = lamp_local_time::LocalTime::System(
        lamp_local_time::SystemZone::open(
            root.path().join("localtime"),
            root.path().join("timezone"),
        )
        .unwrap(),
    );
    let engine = Engine::open_following(Disk::default(), nameless, &at, STORE)
        .expect("a system zone with no known name does not stop the part");
    assert_eq!(engine.zone_fallback(), Some(("system", "Etc/UTC")));
    let engine = Engine::open_with_system(Disk::default(), "system", unreadable, None, &at, STORE)
        .expect("an unreadable system zone does not stop the part");
    assert_eq!(engine.zone_fallback(), Some(("system", "Etc/UTC")));
    // A zone it knows is no fallback.
    let engine = Engine::open(Disk::default(), "Etc/UTC", &at, STORE).unwrap();
    assert_eq!(engine.zone_fallback(), None);
}

/// When an item was set (the person's call, 30 Oct: "when did I set that
/// reminder?"): `ListSet` says it, from the item's stored creation time, in
/// the lamp's zone, its day against now's. 14 Oct 23:30 UTC is 06:30 on 15
/// Oct in Ho Chi Minh City: the day is the lamp's, not UTC's. With now's
/// wall clock untrusted, the day is its date, never "today" on a guess.
/// `List` and `Get`, which the delivery ledger reads strictly, are as they
/// were.
#[test]
fn a_list_set_item_says_when_it_was_set_in_the_lamps_zone() {
    // WALL is 14 Oct 08:00 UTC; 23:30 UTC is 15.5 hours on.
    let at = now(15 * 3_600_000 + 1_800_000);
    let mut e = Engine::open(Disk::default(), "Asia/Ho_Chi_Minh", &at, STORE).unwrap();
    set(&mut e, &at, 86_400 * 3);
    let listed = |e: &mut Engine<Disk>, at: &ClockSample| {
        e.answer(
            Request::ListSet {
                asker: None,
                after_id: 0,
                limit: 8,
                pending_only: false,
            },
            at,
        )
        .unwrap()["items"][0]
            .clone()
    };
    let item = listed(&mut e, &at);
    assert_eq!(
        (item["set_local"].as_str(), item["set_day"].as_str()),
        (
            Some("2026-10-15T06:30:00+07:00[Asia/Ho_Chi_Minh]"),
            Some("today")
        ),
        "{item}"
    );
    let mut plain = item.as_object().unwrap().clone();
    plain.remove("set_local");
    plain.remove("set_day");
    assert_eq!(
        plain.remove("set_unix_ms"),
        Some(json!(WALL + 15 * 3_600_000 + 1_800_000))
    );
    assert_eq!(Value::Object(plain), get(&mut e, &at, 1));
    let list = e
        .answer(
            Request::List {
                after_id: 0,
                limit: 8,
                pending_only: false,
            },
            &at,
        )
        .unwrap();
    assert!(list["items"][0].get("set_local").is_none(), "{list}");
    let day = |e: &mut Engine<Disk>, at: &ClockSample| listed(e, at)["set_day"].clone();
    assert_eq!(day(&mut e, &now(39 * 3_600_000 + 1_800_000)), "yesterday");
    assert_eq!(day(&mut e, &now(63 * 3_600_000 + 1_800_000)), "2026-10-15");
    let mut untrusted = now(64 * 3_600_000);
    untrusted.wall_trusted = false;
    assert_eq!(day(&mut e, &untrusted), "2026-10-15");
}

const ANN: &str = "p-0123456789abcdef";
const BO: &str = "p-fedcba9876543210";

/// A reminder `person` set, due at 08:10, 600 s after [`WALL`]: a
/// reminder, since every line sees the room's timers.
fn set_by(engine: &mut Engine<Disk>, at: &ClockSample, text: &str, person: Option<&str>) -> u64 {
    set_when(
        engine,
        at,
        text,
        person,
        When::At {
            time: "08:10".into(),
            day: None,
            zone: None,
            fold: None,
        },
    )
}
/// A timer `person` set, due 600 s from `at`.
fn timer_by(engine: &mut Engine<Disk>, at: &ClockSample, text: &str, person: Option<&str>) -> u64 {
    set_when(engine, at, text, person, When::After { seconds: 600 })
}
fn set_when(
    engine: &mut Engine<Disk>,
    at: &ClockSample,
    text: &str,
    person: Option<&str>,
    when: When,
) -> u64 {
    engine.tick(at).unwrap();
    engine
        .answer(
            Request::Set {
                guard: guard(engine),
                text: text.into(),
                when,
                person: person.map(str::to_owned),
            },
            at,
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}
fn asker(person: Option<&str>, owner: bool) -> Asker {
    Asker {
        person: person.map(str::to_owned),
        owner,
    }
}
fn listed(engine: &mut Engine<Disk>, at: &ClockSample, asker: Option<Asker>) -> Vec<String> {
    let page = engine
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker,
            },
            at,
        )
        .unwrap();
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap().to_owned())
        .collect()
}

/// #4527's P2, the first builder's (a): each reminder keeps who set it; a
/// line hears only its own, and one set with nobody sure is the room's,
/// heard on every line (the first builder on #4710, 3 Nov 10:09). A request
/// with no asker (the app, the due reminder's own reads) sees them all.
#[test]
fn each_line_lists_only_the_reminders_it_set_and_every_line_hears_the_rooms() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    set_by(&mut e, &at, "Ann's pills", Some(ANN));
    set_by(&mut e, &at, "Bo's call", Some(BO));
    set_by(&mut e, &at, "Water the plant", None);
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Ann's pills", "Water the plant"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Bo's call", "Water the plant"]
    );
    // The owner is Ann: hers and the room's, never Bo's.
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), true))),
        ["Ann's pills", "Water the plant"]
    );
    // An unknown face, or the room's line: the room's, nothing of anyone's.
    assert_eq!(
        listed(&mut e, &at, Some(asker(None, false))),
        ["Water the plant"]
    );
    assert_eq!(listed(&mut e, &at, None).len(), 3);
    // Its count of what is set and not yet due is the line's own.
    let page = e
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: true,
                asker: Some(asker(Some(BO), false)),
            },
            &at,
        )
        .unwrap();
    assert_eq!(page["not_yet_due"], 2);
}

/// The memory owner's option 2 (4 Nov, 29e7's "only call your dad"): the
/// room's line is told how many it may not see, a count and nothing of them;
/// a person's line counts none.
#[test]
fn the_rooms_line_is_told_only_how_many_belong_to_someone() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    set_by(&mut e, &at, "Call your mom", Some(ANN));
    set_by(&mut e, &at, "Call your dad", None);
    let page = |e: &mut Engine<Disk>, asker: Asker| {
        e.answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker: Some(asker),
            },
            &at,
        )
        .unwrap()
    };
    let room = page(&mut e, asker(None, false));
    assert_eq!(room["items"].as_array().unwrap().len(), 1);
    assert_eq!(room["hidden"], 1);
    let text = room.to_string();
    assert!(!text.contains("mom") && !text.contains(ANN), "{text}");
    assert_eq!(page(&mut e, asker(Some(ANN), false))["hidden"], 0);
    // No asker, the app's: every item, and no count.
    let all = e
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker: None,
            },
            &at,
        )
        .unwrap();
    assert!(all.get("hidden").is_none());
}

/// The first builder on #4710 (3 Nov 10:09), from lamp-9adf: "remind me to
/// drink water every day at five" set with nobody known in view, then "stop
/// reminding me to drink water" a few seconds later found nothing. A
/// reminder set while nobody known is in view is the room's: anyone in the
/// room hears it listed and stops it by voice; a known person's stays theirs.
#[test]
fn the_room_line_that_set_a_reminder_hears_and_cancels_it_and_never_a_persons() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    let ann = set_by(&mut e, &at, "Ann's pills", Some(ANN));
    let water = set_by(&mut e, &at, "Drink water", None);
    let room = asker(None, false);
    assert_eq!(listed(&mut e, &at, Some(room.clone())), ["Drink water"]);
    let refused = e
        .answer(
            Request::Cancel {
                guard: guard(&e),
                id: ann,
                asker: Some(room.clone()),
            },
            &at,
        )
        .unwrap_err();
    assert_eq!(refused.code, "not_found");
    e.answer(
        Request::Cancel {
            guard: guard(&e),
            id: water,
            asker: Some(room.clone()),
        },
        &at,
    )
    .unwrap();
    assert!(listed(&mut e, &at, Some(room)).is_empty());
    assert_eq!(listed(&mut e, &at, None), ["Ann's pills"]);
}

#[test]
fn a_cancel_or_snooze_by_another_persons_id_is_not_found_and_changes_nothing() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    let ann = set_by(&mut e, &at, "Ann's pills", Some(ANN));
    let room = set_by(&mut e, &at, "Water the plant", None);
    for (id, guest) in [(ann, asker(Some(BO), false)), (ann, asker(None, false))] {
        let refused = e
            .answer(
                Request::Cancel {
                    guard: guard(&e),
                    id,
                    asker: Some(guest.clone()),
                },
                &at,
            )
            .unwrap_err();
        assert_eq!(refused.code, "not_found", "{id} {guest:?}");
    }
    let due_at = now(600_000);
    e.tick(&due_at).unwrap();
    let delivery = get(&mut e, &due_at, ann)["delivery"].as_u64().unwrap();
    let refused = e
        .answer(
            Request::Snooze {
                guard: guard(&e),
                id: ann,
                delivery,
                seconds: 60,
                asker: Some(asker(Some(BO), false)),
            },
            &due_at,
        )
        .unwrap_err();
    assert_eq!(refused.code, "not_found");
    assert_eq!(listed(&mut e, &due_at, None).len(), 2);
    // Ann snoozes her own, and the owner cancels the room's.
    e.answer(
        Request::Snooze {
            guard: guard(&e),
            id: ann,
            delivery,
            seconds: 60,
            asker: Some(asker(Some(ANN), false)),
        },
        &due_at,
    )
    .unwrap();
    e.answer(
        Request::Cancel {
            guard: guard(&e),
            id: room,
            asker: Some(asker(Some(ANN), true)),
        },
        &due_at,
    )
    .unwrap();
    assert_eq!(listed(&mut e, &due_at, None), ["Ann's pills"]);
}

/// The room's timers (option 1 on #4656's after-merge P2), and since the
/// first builder on #4710 (3 Nov 10:09) the room's reminders too: a line with
/// no face seen, at the stove, hears and cancels a timer or reminder set by
/// nobody, short or long. It never hears a person's timer, reminder, or
/// relative reminder.
#[test]
fn the_room_hears_every_one_set_by_nobody_and_never_a_persons() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    let weeks = || When::After {
        seconds: 21 * 86_400,
    };
    let pasta = timer_by(&mut e, &at, "Pasta", None);
    let tea = timer_by(&mut e, &at, "Tea", Some(ANN));
    let pills = set_by(&mut e, &at, "Ann's pills", Some(ANN));
    set_when(&mut e, &at, "Ann's prescription", Some(ANN), weeks());
    set_when(&mut e, &at, "Renew the visa", None, weeks());
    // At the bound and just past it.
    set_when(&mut e, &at, "Roast", None, When::After { seconds: 7_200 });
    set_when(&mut e, &at, "Bread", None, When::After { seconds: 7_201 });
    let room = asker(None, false);
    let nobodys = ["Pasta", "Renew the visa", "Roast", "Bread"];
    assert_eq!(listed(&mut e, &at, Some(room.clone())), nobodys);
    assert_eq!(listed(&mut e, &at, Some(asker(Some(BO), false))), nobodys);
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        [
            "Pasta",
            "Tea",
            "Ann's pills",
            "Ann's prescription",
            "Renew the visa",
            "Roast",
            "Bread"
        ]
    );
    // The owner's line hears every one set by nobody, as every line does.
    assert_eq!(
        listed(&mut e, &at, Some(asker(None, true))),
        ["Pasta", "Renew the visa", "Roast", "Bread"]
    );
    let cancel = |e: &mut Engine<Disk>, id: u64| {
        e.answer(
            Request::Cancel {
                guard: guard(e),
                id,
                asker: Some(room.clone()),
            },
            &at,
        )
    };
    for id in [tea, pills] {
        assert_eq!(cancel(&mut e, id).unwrap_err().code, "not_found", "{id}");
    }
    cancel(&mut e, pasta).unwrap();
    assert_eq!(
        listed(&mut e, &at, Some(room.clone())),
        ["Renew the visa", "Roast", "Bread"]
    );
    // A person's timer keeps its setter: Ann's forget takes it with hers.
    let forgot = e
        .answer(
            Request::Forget {
                guard: guard(&e),
                person: ANN.into(),
            },
            &at,
        )
        .unwrap();
    assert_eq!(forgot["forgotten"], 3);
    assert_eq!(
        listed(&mut e, &at, None),
        ["Renew the visa", "Roast", "Bread"]
    );
}

#[test]
fn a_persons_forget_cancels_every_reminder_they_set_and_survives_a_reopen() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    set_by(&mut e, &at, "Ann's pills", Some(ANN));
    set_by(&mut e, &at, "Ann's call", Some(ANN));
    set_by(&mut e, &at, "Bo's call", Some(BO));
    set_by(&mut e, &at, "Water the plant", None);
    let forgot = e
        .answer(
            Request::Forget {
                guard: guard(&e),
                person: ANN.into(),
            },
            &at,
        )
        .unwrap();
    assert_eq!(forgot["forgotten"], 2);
    assert_eq!(listed(&mut e, &at, None), ["Bo's call", "Water the plant"]);
    let refused = e
        .answer(
            Request::Forget {
                guard: guard(&e),
                person: "Ann".into(),
            },
            &at,
        )
        .unwrap_err();
    assert_eq!(refused.code, "bad_request");
    drop(e);
    let mut e = open(disk, &at);
    assert_eq!(listed(&mut e, &at, None), ["Bo's call", "Water the plant"]);
    // Bo hears his own and the room's, never Ann's (gone) or another's.
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Bo's call", "Water the plant"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Water the plant"]
    );
}

#[test]
fn a_store_from_before_setters_loads_with_every_item_no_persons_and_keeps_the_setter_on_reopen() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    // A reminder: a short timer set by nobody is every line's.
    set_by(&mut e, &at, "Call Maria", None);
    set_by(&mut e, &at, "Ann's pills", Some(ANN));
    drop(e);
    // The older build's store: no `person` on the item set without one.
    let stored: Value =
        serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    assert!(stored["items"][0].get("person").is_none());
    assert_eq!(stored["items"][1]["person"], ANN);
    let mut e = open(disk, &at);
    // One with no setter is the room's: every line hears it; Ann's only hers.
    assert_eq!(listed(&mut e, &at, Some(asker(None, true))), ["Call Maria"]);
    assert_eq!(
        listed(&mut e, &at, Some(asker(None, false))),
        ["Call Maria"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Call Maria"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Call Maria", "Ann's pills"]
    );
    // A setter that is not a person id is refused, never stored.
    let refused = e
        .answer(
            Request::Set {
                guard: guard(&e),
                text: "x".into(),
                when: When::After { seconds: 60 },
                person: Some("Ann".into()),
            },
            &at,
        )
        .unwrap_err();
    assert_eq!(refused.code, "bad_request");
}

/// `LAMP_SCHEDULE_EVERY` (the first builder's yes on #4710, 2 Nov 17:4x):
/// off, a reminder every so many minutes is refused as a kind this build
/// doesn't take; on, it is kept, its occurrences come one step apart, and
/// "stop reminding me" (a cancel) ends the whole series.
#[test]
fn every_is_refused_with_its_switch_off_and_cancelled_whole_with_it_on() {
    let hourly = When::Every {
        minutes: None,
        hours: Some(1),
        from: None,
        to: None,
        zone: None,
    };
    let disk = Disk::default();
    let schema = |disk: &Disk| {
        let bytes = disk.0.lock().unwrap().bytes.clone().unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()["schema"].clone()
    };
    let mut e = open(disk.clone(), &now(0));
    e.tick(&now(0)).unwrap();
    let refused = e
        .answer(
            Request::Set {
                person: None,
                guard: guard(&e),
                text: "Drink some water".into(),
                when: hourly.clone(),
            },
            &now(0),
        )
        .unwrap_err();
    assert_eq!(refused.code, "bad_request");
    // With no `every` kept, the store is one a build from before reads.
    set(&mut e, &now(0), 60);
    assert_eq!(schema(&disk), 1);
    let r = Request::Cancel {
        asker: None,
        guard: guard(&e),
        id: 1,
    };
    e.answer(r, &now(0)).unwrap();
    e.allow_every(true);
    create(&mut e, &now(0), hourly);
    assert_eq!(schema(&disk), 2);
    // This build reads its own schema 2 store back.
    drop(e);
    let mut e = open(disk.clone(), &now(0));
    e.allow_every(true);
    let first = get(&mut e, &now(0), 2)["due_unix_ms"].as_i64().unwrap();
    assert_eq!(first - WALL, 3_600_000);
    // Due, said, acknowledged: the next one is an hour on.
    let mut at = now(1000);
    at.unix_ms = first;
    e.tick(&at).unwrap();
    assert_eq!(due(&e, &at).oldest.unwrap().id, 2);
    ack(&mut e, &at, 2, 1);
    let second = get(&mut e, &at, 2)["due_unix_ms"].as_i64().unwrap();
    assert_eq!(second - first, 3_600_000);
    // "Stop reminding me": the series is cancelled, nothing more is due.
    e.tick(&at).unwrap();
    let r = Request::Cancel {
        asker: None,
        guard: guard(&e),
        id: 2,
    };
    assert_eq!(e.answer(r, &at).unwrap()["cancelled"], true);
    at.unix_ms = second + 7_200_000;
    at.boot_ms += 10_000;
    e.tick(&at).unwrap();
    assert!(due(&e, &at).oldest.is_none());
}

// 29e7, 2 Nov: reminders said are kept in a said record, one said late
// waits there for an answer, and the same set twice in a moment is one.

const HOUR: u64 = 3_600_000;

fn said(engine: &mut Engine<Disk>, at: &ClockSample, asker: Option<Asker>) -> Vec<Value> {
    engine
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker,
            },
            at,
        )
        .unwrap()["said"]
        .as_array()
        .unwrap()
        .clone()
}
/// Every pending item acknowledged at `at`, oldest first, as the voice says
/// them.
fn ack_all(engine: &mut Engine<Disk>, at: &ClockSample) -> usize {
    engine.tick(at).unwrap();
    let pending: Vec<(u64, u64)> = engine
        .answer(
            Request::List {
                after_id: 0,
                limit: 8,
                pending_only: true,
            },
            at,
        )
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_u64().unwrap(), i["delivery"].as_u64().unwrap()))
        .collect();
    for (id, delivery) in &pending {
        ack(engine, at, *id, *delivery);
    }
    pending.len()
}
fn texts_of(said: &[Value]) -> Vec<&str> {
    said.iter().map(|s| s["text"].as_str().unwrap()).collect()
}

/// The 29e7 sequence: four one-time reminders due while the microphones
/// were closed, said late, then "what reminders did I set earlier?": the
/// list has none set, and all four as said, waiting for an answer. On
/// 29e7 they were 13 to 38 hours late, too late to be said now (#5258):
/// here they are 15 minutes to two hours late.
#[test]
fn four_late_reminders_said_are_kept_as_said_and_wait_for_an_answer() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    for (text, seconds) in [
        ("Send the report", 3 * 3_600),
        ("Buy the gift", 4 * 3_600),
        ("Drink water", 4 * 3_600 + 60),
        ("Water timer", 4 * 3_600 + 120),
    ] {
        set_when(&mut e, &at, text, Some(ANN), When::After { seconds });
    }
    let opened = now(5 * HOUR);
    assert_eq!(ack_all(&mut e, &opened), 4);
    let list = e
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker: Some(asker(Some(ANN), false)),
            },
            &opened,
        )
        .unwrap();
    assert_eq!(list["items"], json!([]));
    let said = list["said"].as_array().unwrap();
    // Newest first; all said at the same moment, so the last acked first.
    assert_eq!(
        texts_of(said),
        [
            "Water timer",
            "Drink water",
            "Buy the gift",
            "Send the report"
        ]
    );
    for one in said {
        assert_eq!(one["waiting_for_answer"], true, "{one}");
        assert_eq!(one["on_time"], false);
        assert!(one["late_by_ms"].as_u64().unwrap() > lamp_schedule::engine::SAID_LATE_MS);
        assert!(one["said_local"].as_str().is_some() && one["said_day"].as_str().is_some());
    }
    // They survive a restart.
    drop(e);
    let mut e = open(disk, &opened);
    assert_eq!(said_of(&mut e, &opened, ANN).len(), 4);
}
fn said_of(engine: &mut Engine<Disk>, at: &ClockSample, person: &str) -> Vec<Value> {
    said(engine, at, Some(asker(Some(person), false)))
}

#[test]
fn a_line_after_a_late_one_notices_it_and_the_grace_ends_the_wait() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    // Set for three hours, a reminder, not a kitchen timer: said an hour
    // late, not too late (#5258).
    set_when(
        &mut e,
        &at,
        "Stretch",
        Some(ANN),
        When::After { seconds: 3 * 3_600 },
    );
    let late = now(4 * HOUR);
    assert_eq!(ack_all(&mut e, &late), 1);
    assert_eq!(said_of(&mut e, &late, ANN)[0]["waiting_for_answer"], true);
    let revision = e.revision();
    let noticed = e.answer(Request::Noticed {}, &late).unwrap();
    assert_eq!(noticed["noticed"], 1);
    assert_eq!(e.revision(), revision + 1);
    let kept = said_of(&mut e, &late, ANN);
    assert_eq!(kept[0]["waiting_for_answer"], false);
    assert_eq!(kept[0]["text"], "Stretch");
    // Noticing again writes nothing.
    assert_eq!(e.answer(Request::Noticed {}, &late).unwrap()["noticed"], 0);
    assert_eq!(e.revision(), revision + 1);

    // One nobody answers stops waiting after the grace.
    set_when(
        &mut e,
        &late,
        "Pills",
        Some(ANN),
        When::After { seconds: 3 * 3_600 },
    );
    let later = now(8 * HOUR);
    ack_all(&mut e, &later);
    assert_eq!(said_of(&mut e, &later, ANN)[0]["waiting_for_answer"], true);
    let after_grace = now(10 * HOUR + 1);
    assert_eq!(
        said_of(&mut e, &after_grace, ANN)[0]["waiting_for_answer"],
        false
    );
    assert_eq!(
        e.answer(Request::Noticed {}, &after_grace).unwrap()["noticed"],
        0
    );
}

#[test]
fn one_said_on_time_is_kept_as_said_on_time_and_waits_for_nothing() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    timer_by(&mut e, &at, "Tea", None);
    let due = now(600_000);
    assert_eq!(ack_all(&mut e, &due), 1);
    let kept = said(&mut e, &due, None);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["on_time"], true);
    assert_eq!(kept[0]["waiting_for_answer"], false);
    assert_eq!(kept[0]["kind"], "timer");
    // A repeat is not one-time: it goes on, and is not kept as said.
    create(
        &mut e,
        &due,
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    ack_all(&mut e, &now(HOUR + 600_000));
    assert_eq!(said(&mut e, &now(HOUR + 600_000), None).len(), 1);
}

#[test]
fn a_line_hears_its_own_and_the_rooms_said_ones_and_a_forget_takes_theirs() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    set_when(
        &mut e,
        &at,
        "Ann's pills",
        Some(ANN),
        When::After { seconds: 3 * 3_600 },
    );
    set_when(
        &mut e,
        &at,
        "Bo's call",
        Some(BO),
        When::After { seconds: 3 * 3_600 },
    );
    set_by(&mut e, &at, "Water the plant", None);
    // The room's reminder and timer: every line hears them.
    timer_by(&mut e, &at, "Pasta", None);
    // Each said when it is due: hours late, the room's two would be too
    // late to be said at all (#5258).
    ack_all(&mut e, &now(600_000));
    ack_all(&mut e, &now(4 * HOUR));
    let at = now(4 * HOUR);
    let mut ann = texts_of(&said_of(&mut e, &at, ANN))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    ann.sort();
    assert_eq!(ann, ["Ann's pills", "Pasta", "Water the plant"]);
    let mut bo = texts_of(&said_of(&mut e, &at, BO))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    bo.sort();
    assert_eq!(bo, ["Bo's call", "Pasta", "Water the plant"]);
    let mut owner = texts_of(&said(&mut e, &at, Some(asker(None, true))))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    owner.sort();
    assert_eq!(owner, ["Pasta", "Water the plant"]);
    assert_eq!(said(&mut e, &at, None).len(), 4);
    // An unknown voice hears only the room's.
    let mut room = texts_of(&said(&mut e, &at, Some(asker(None, false))))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    room.sort();
    assert_eq!(room, ["Pasta", "Water the plant"]);
    e.answer(
        Request::Forget {
            guard: guard(&e),
            person: ANN.into(),
        },
        &at,
    )
    .unwrap();
    let mut ann = texts_of(&said_of(&mut e, &at, ANN))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    ann.sort();
    assert_eq!(ann, ["Pasta", "Water the plant"]);
    assert_eq!(said(&mut e, &at, None).len(), 3);
    drop(e);
    let stored = String::from_utf8(disk.0.lock().unwrap().bytes.clone().unwrap()).unwrap();
    assert!(!stored.contains("Ann's pills"), "{stored}");
}

#[test]
fn the_said_record_keeps_the_last_twenty_and_none_past_a_week() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    for n in 0..25u64 {
        let at = now(n * 60_000);
        set_when(
            &mut e,
            &at,
            &format!("Timer {n}"),
            None,
            When::After { seconds: 1 },
        );
        ack_all(&mut e, &now(n * 60_000 + 1_000));
    }
    let at = now(30 * 60_000);
    let kept = said(&mut e, &at, None);
    assert_eq!(kept.len(), lamp_schedule::engine::SAID_KEPT);
    assert_eq!(kept[0]["text"], "Timer 24");
    assert_eq!(kept[19]["text"], "Timer 5");
    // A plain list, the app's, is as it was: no said record.
    let plain = e
        .answer(
            Request::List {
                after_id: 0,
                limit: 8,
                pending_only: false,
            },
            &at,
        )
        .unwrap();
    assert!(plain.get("said").is_none(), "{plain}");
    // Nor a later page.
    let later = e
        .answer(
            Request::ListSet {
                after_id: 1,
                limit: 8,
                pending_only: false,
                asker: None,
            },
            &at,
        )
        .unwrap();
    assert!(later.get("said").is_none(), "{later}");
    let week = now(8 * 24 * HOUR);
    assert!(said(&mut e, &week, None).is_empty());
}

#[test]
fn a_store_without_a_said_record_or_with_one_entry_unread_loads() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    timer_by(&mut e, &at, "Tea", None);
    drop(e);
    let mut stored: Value =
        serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    assert!(stored.get("said").is_none(), "{stored}");
    let mut e = open(disk.clone(), &at);
    ack_all(&mut e, &now(600_000));
    drop(e);
    stored = serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    assert_eq!(stored["said"].as_array().unwrap().len(), 1);
    // A newer build's entry this build can't read is dropped, not the store.
    stored["said"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":1,"text":7}));
    disk.0.lock().unwrap().bytes = Some(serde_json::to_vec(&stored).unwrap());
    let mut e = open(disk, &now(600_000));
    assert_eq!(said(&mut e, &now(600_000), None).len(), 1);
}

/// The engine sets what it is asked to, twice if asked twice: the same
/// timer asked for again in a moment is the voice's adapter's to catch
/// (29e7's ids 8 and 9), by each item's `set_unix_ms`.
#[test]
fn list_set_says_when_each_was_set_to_the_millisecond() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    timer_by(&mut e, &at, "Drink water", Some(ANN));
    timer_by(&mut e, &now(29_000), "Drink water", Some(ANN));
    let page = e
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker: Some(asker(Some(ANN), false)),
            },
            &now(30_000),
        )
        .unwrap();
    let set: Vec<_> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["set_unix_ms"].as_i64().unwrap())
        .collect();
    assert_eq!(set, [WALL, WALL + 29_000]);
}

// #5258, #5219: a reminder or timer too late to mean anything is dropped,
// never said, and the store keeps it as dropped late.

use lamp_schedule::engine::{REMINDER_TOO_LATE_MS, TIMER_TOO_LATE_MS};

/// The store's record of those dropped too late, as written.
fn dropped_late(disk: &Disk) -> Vec<Value> {
    let bytes = disk.0.lock().unwrap().bytes.clone().unwrap();
    let stored: Value = serde_json::from_slice(&bytes).unwrap();
    stored["dropped_late"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_timer_ten_minutes_late_still_rings_and_one_a_second_more_is_dropped() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 60);
    let ten = now(60_000 + TIMER_TOO_LATE_MS);
    e.tick(&ten).unwrap();
    let oldest = due(&e, &ten).oldest.unwrap();
    assert_eq!(oldest.id, 1);
    assert_eq!(oldest.late_by_ms, Some(TIMER_TOO_LATE_MS));
    assert!(dropped_late(&disk).is_empty());

    let over = now(60_000 + TIMER_TOO_LATE_MS + 1_000);
    e.tick(&over).unwrap();
    assert!(due(&e, &over).oldest.is_none());
    assert_eq!(e.counts(&over), (0, 0, 0));
    let dropped = dropped_late(&disk);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["id"], 1);
    assert_eq!(dropped[0]["timer"], true);
    assert_eq!(dropped[0]["late_by_ms"], TIMER_TOO_LATE_MS + 1_000);
    assert_eq!(dropped[0]["waiting"], false);
    // Never told as said.
    assert!(said(&mut e, &over, None).is_empty());
}

#[test]
fn a_reminder_two_hours_late_still_rings_and_one_a_second_more_is_dropped() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    // 08:10 in the lamp's zone (now(0) is 08:00 UTC).
    let id = set_by(&mut e, &now(0), "Water the plant", None);
    // Set for three hours from now: a reminder, kept as one, not a timer.
    let long = set_when(
        &mut e,
        &now(0),
        "Renew the prescription",
        None,
        When::After { seconds: 3 * 3_600 },
    );
    let due_ms = 600_000;
    let two = now(due_ms + REMINDER_TOO_LATE_MS);
    e.tick(&two).unwrap();
    let oldest = due(&e, &two).oldest.unwrap();
    assert_eq!(oldest.id, id);
    assert_eq!(oldest.late_by_ms, Some(REMINDER_TOO_LATE_MS));

    let over = now(due_ms + REMINDER_TOO_LATE_MS + 1_000);
    e.tick(&over).unwrap();
    assert_eq!(e.counts(&over).0, 1, "only the three-hour one is left");
    let dropped = dropped_late(&disk);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["id"], id);
    assert_eq!(dropped[0]["timer"], false);
    assert_eq!(dropped[0]["late_by_ms"], REMINDER_TOO_LATE_MS + 1_000);

    // The three-hour one half an hour late: a timer's ten minutes would
    // have dropped it; a reminder's two hours keep it.
    let half = now(3 * HOUR + 30 * 60_000);
    e.tick(&half).unwrap();
    let oldest = due(&e, &half).oldest.unwrap();
    assert_eq!(oldest.id, long);
    assert_eq!(oldest.late_by_ms, Some(30 * 60_000));
}

/// Due before midnight in the lamp's zone and offered after it: from an
/// earlier day, it is dropped, however few minutes late. The same lateness
/// before midnight is said. The lamp's zone, not UTC, draws the line.
#[test]
fn a_reminder_due_before_local_midnight_is_dropped_after_it() {
    let disk = Disk::default();
    // 15:00 in Ho Chi Minh City (now(0) is 08:00 UTC); midnight there is
    // 17:00 UTC.
    let mut e = Engine::open(disk.clone(), "Asia/Ho_Chi_Minh", &now(0), STORE).unwrap();
    let at = |time: &str| When::At {
        time: time.into(),
        day: None,
        zone: None,
        fold: None,
    };
    let earlier = set_when(&mut e, &now(0), "Lock the door", None, at("23:40"));
    let later = set_when(&mut e, &now(0), "Take the pills", None, at("23:50"));
    // 23:55: both late, on their own day, so both are said.
    let before = now(8 * HOUR + 55 * 60_000);
    e.tick(&before).unwrap();
    assert_eq!(due(&e, &before).pending, 2);
    assert_eq!(due(&e, &before).oldest.unwrap().id, earlier);

    // 00:05 the next day: 25 and 15 minutes late, from yesterday.
    let after = now(9 * HOUR + 5 * 60_000);
    e.tick(&after).unwrap();
    assert!(due(&e, &after).oldest.is_none());
    let dropped: Vec<_> = dropped_late(&disk)
        .iter()
        .map(|d| (d["id"].as_u64().unwrap(), d["late_by_ms"].as_u64().unwrap()))
        .collect();
    assert_eq!(dropped, [(earlier, 25 * 60_000), (later, 15 * 60_000)]);
}

/// lamp-e82d (#5258): a one-minute timer due 21:49:37 rang into a sleep
/// mute, was never proven said, and came back 18.4 hours later as the lamp
/// woke. Waiting out the mute, still unacknowledged, it is dropped once it
/// is ten minutes late, so the waking lamp finds nothing to ring. One that
/// comes due while Schedule itself was down is dropped on its first look.
#[test]
fn a_timer_waiting_out_a_sleep_mute_is_dropped_not_rung_when_the_lamp_wakes() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 60);
    // Due, offered and never acknowledged: the voice is muted.
    let muted = now(60_000 + 5 * 60_000);
    e.tick(&muted).unwrap();
    assert_eq!(due(&e, &muted).pending, 1);
    // The lamp wakes 18.4 hours on.
    let woke = now(60_000 + 66_240_000);
    e.tick(&woke).unwrap();
    assert_eq!(due(&e, &woke).pending, 0);
    assert!(due(&e, &woke).oldest.is_none());
    assert_eq!(dropped_late(&disk).len(), 1);
    // It stays dropped across a restart.
    drop(e);
    let mut e = open(disk.clone(), &woke);
    e.tick(&woke).unwrap();
    assert_eq!(e.counts(&woke), (0, 0, 0));

    // Set, then Schedule is down past its time: its first look drops it.
    let set_at = now(70_000_000);
    set(&mut e, &set_at, 60);
    drop(e);
    let mut back = now(70_000_000 + 60_000 + HOUR);
    back.boot_id = "boot-b".into();
    back.boot_ms = 5_000;
    let mut e = open(disk.clone(), &back);
    e.tick(&back).unwrap();
    assert!(due(&e, &back).oldest.is_none());
    let dropped = dropped_late(&disk);
    assert_eq!(dropped.len(), 2);
    assert_eq!(dropped[1]["late_by_ms"], HOUR);
}

/// A daily reminder missed by more than two hours: that day's is dropped,
/// and the next goes on, as an acknowledged one does.
#[test]
fn a_repeat_too_late_drops_only_that_occurrence() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    create(
        &mut e,
        &now(0),
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
    );
    let first = get(&mut e, &now(0), 1)["due_unix_ms"].as_i64().unwrap();
    drop(e);
    let mut at = now(0);
    at.boot_id = "boot-b".into();
    at.boot_ms = 1_000;
    at.unix_ms = first + 3 * HOUR as i64;
    let mut e = open(disk.clone(), &at);
    e.tick(&at).unwrap();
    assert!(due(&e, &at).oldest.is_none());
    let item = get(&mut e, &at, 1);
    assert_eq!(item["delivery"], 2);
    assert_eq!(item["due_unix_ms"].as_i64(), Some(first + 86_400_000));
    assert_eq!(dropped_late(&disk)[0]["late_by_ms"], 3 * HOUR);
}

/// A clock not yet trusted tells no lateness: nothing is dropped on it.
#[test]
fn nothing_is_dropped_on_a_clock_not_yet_trusted() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set_by(&mut e, &now(0), "Water the plant", None);
    drop(e);
    let mut at = now(10 * HOUR);
    at.boot_id = "boot-b".into();
    at.boot_ms = 1_000;
    at.wall_trusted = false;
    let mut e = open(disk.clone(), &at);
    e.tick(&at).unwrap();
    assert_eq!(e.counts(&at).0, 1);
    assert!(
        disk.0
            .lock()
            .unwrap()
            .bytes
            .as_ref()
            .is_some_and(|b| { !String::from_utf8_lossy(b).contains("dropped_late") })
    );
}

/// #5447's P3: a timer rung just before its cut-off and acknowledged just
/// after it is kept as said, not dropped: read for its offer 9 min 59 s
/// late, acknowledged at 10 min 1 s. One never offered is dropped at 10 min
/// 1 s, and one offered but never acknowledged is dropped a minute after
/// its last read before the cut-off; reads after it don't renew that.
#[test]
fn a_timer_offered_before_its_cut_off_and_acked_after_it_is_kept_as_said() {
    use lamp_schedule::engine::OFFERED_HOLD_MS;
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    set(&mut e, &now(0), 60);
    set(&mut e, &now(0), 60);
    let offered = now(60_000 + TIMER_TOO_LATE_MS - 1_000);
    assert_eq!(get(&mut e, &offered, 1)["state"], "pending");
    let after = now(60_000 + TIMER_TOO_LATE_MS + 1_000);
    ack(&mut e, &after, 1, 1);
    let kept = said(&mut e, &after, None);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["id"], 1);
    assert_eq!(kept[0]["late_by_ms"], TIMER_TOO_LATE_MS + 1_000);
    let dropped = dropped_late(&disk);
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(dropped[0]["id"], 2, "the one never offered");

    // Offered just before the cut-off and never acknowledged: read again
    // after it, still held for a minute from the read before it, then
    // dropped.
    let at = now(HOUR);
    set(&mut e, &at, 60);
    let due = HOUR + 60_000;
    let before = now(due + TIMER_TOO_LATE_MS - 1_000);
    get(&mut e, &before, 3);
    get(&mut e, &now(due + TIMER_TOO_LATE_MS + 30_000), 3);
    let held = now(due + TIMER_TOO_LATE_MS - 1_000 + OFFERED_HOLD_MS);
    e.tick(&held).unwrap();
    assert_eq!(due_of(&e, &held), Some(3));
    let gone = now(due + TIMER_TOO_LATE_MS + OFFERED_HOLD_MS);
    e.tick(&gone).unwrap();
    assert_eq!(due_of(&e, &gone), None);
    assert_eq!(dropped_late(&disk).len(), 2);
}
fn due_of(engine: &Engine<Disk>, at: &ClockSample) -> Option<u64> {
    due(engine, at).oldest.map(|oldest| oldest.id)
}

// C68, lamp-06c1 on 11 Nov at 01:30: "Call the plumber." was said, "snooze
// it" came 9 s later, and the reminder, gone from the set list once said,
// could not be snoozed. One just said may be snoozed or cancelled for
// `SAID_SNOOZABLE_MS` after it, across a restart; never after.

fn snooze_said(
    engine: &mut Engine<Disk>,
    at: &ClockSample,
    id: u64,
    delivery: u64,
    asker: Option<Asker>,
) -> Result<Value, lamp_schedule::engine::Error> {
    engine.answer(
        Request::Snooze {
            guard: guard(engine),
            id,
            delivery,
            seconds: 300,
            asker,
        },
        at,
    )
}

#[test]
fn a_one_time_reminder_just_said_is_snoozed_again_within_its_window_across_a_restart() {
    use lamp_schedule::engine::SAID_SNOOZABLE_MS;
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    let id = set(&mut e, &now(0), 60)["id"].as_u64().unwrap();
    assert_eq!(ack_all(&mut e, &now(61_000)), 1);
    let listed = said(&mut e, &now(70_000), None);
    assert_eq!(listed[0]["can_snooze_or_cancel"], true, "{listed:?}");
    assert_eq!(listed[0]["delivery"], 1);
    assert_eq!(
        listed[0]["until_unix_ms"],
        listed[0]["said_unix_ms"].as_i64().unwrap() + SAID_SNOOZABLE_MS
    );
    // The service restarts 9 s after it was said: still snoozable.
    let mut e = open(disk.clone(), &now(70_000));
    let snoozed = snooze_said(&mut e, &now(70_000), id, 1, None).unwrap();
    assert_eq!(snoozed["snoozed"], true, "{snoozed}");
    assert_eq!(snoozed["delivery"], 2);
    assert_eq!(snoozed["due_unix_ms"], WALL + 370_000);
    assert_eq!(get(&mut e, &now(70_000), id)["snoozed"], true);
    // Once: the said record no longer offers it.
    assert!(said(&mut e, &now(71_000), None)[0]["can_snooze_or_cancel"].is_null());
    // Due again five minutes on, and said once more.
    e.tick(&now(370_000)).unwrap();
    assert_eq!(due_of(&e, &now(370_000)), Some(id));
    // A second store opens what this one wrote, receipt and all.
    open(disk, &now(371_000));
}

#[test]
fn a_one_time_reminder_said_longer_ago_than_its_window_is_never_snoozed_or_cancelled() {
    use lamp_schedule::engine::SAID_SNOOZABLE_MS;
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    let id = set(&mut e, &now(0), 60)["id"].as_u64().unwrap();
    ack_all(&mut e, &now(61_000));
    let past = now(61_000 + SAID_SNOOZABLE_MS as u64 + 1);
    let refused = snooze_said(&mut e, &past, id, 1, None).unwrap_err();
    assert_eq!(refused.code, "not_found");
    let refused = e
        .answer(
            Request::Cancel {
                guard: guard(&e),
                id,
                asker: None,
            },
            &past,
        )
        .unwrap_err();
    assert_eq!(refused.code, "not_found");
    assert!(said(&mut e, &past, None)[0]["can_snooze_or_cancel"].is_null());
    open(disk, &past);
}

#[test]
fn a_one_time_reminder_just_said_is_cancelled_for_good_and_only_by_its_own_line() {
    let disk = Disk::default();
    let mut e = open(disk.clone(), &now(0));
    e.tick(&now(0)).unwrap();
    let id = e
        .answer(
            Request::Set {
                person: Some(ANN.into()),
                guard: guard(&e),
                text: "Call the plumber".into(),
                when: When::After { seconds: 60 },
            },
            &now(0),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    ack_all(&mut e, &now(61_000));
    // Bo's line, and the room's, neither hear nor change Ann's.
    for other in [asker(Some(BO), false), asker(None, false)] {
        assert!(said(&mut e, &now(65_000), Some(other.clone())).is_empty());
        let refused = snooze_said(&mut e, &now(65_000), id, 1, Some(other.clone())).unwrap_err();
        assert_eq!(refused.code, "not_found");
        let refused = e
            .answer(
                Request::Cancel {
                    guard: guard(&e),
                    id,
                    asker: Some(other),
                },
                &now(65_000),
            )
            .unwrap_err();
        assert_eq!(refused.code, "not_found");
    }
    let ann = Some(asker(Some(ANN), false));
    let cancelled = e
        .answer(
            Request::Cancel {
                guard: guard(&e),
                id,
                asker: ann.clone(),
            },
            &now(66_000),
        )
        .unwrap();
    assert_eq!(cancelled["cancelled"], true, "{cancelled}");
    assert_eq!(cancelled["delivery"], 1);
    // Ended for good: not snoozed after, never due again.
    let refused = snooze_said(&mut e, &now(67_000), id, 1, ann).unwrap_err();
    assert_eq!(refused.code, "not_found");
    e.tick(&now(400_000)).unwrap();
    assert_eq!(due_of(&e, &now(400_000)), None);
    open(disk, &now(400_000));
}
