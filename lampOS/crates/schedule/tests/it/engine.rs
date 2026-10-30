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
    let set = create(&mut e, &later, When::After { seconds: 60 });
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
/// reminder, since every line sees a timer.
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
/// line hears only its own, and one set with nobody sure only on the
/// owner's line. A request with no asker (the app, the due reminder's own
/// reads) sees them all.
#[test]
fn each_line_lists_only_the_reminders_it_set_and_the_owner_also_hears_the_no_person_ones() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    set_by(&mut e, &at, "Ann's pills", Some(ANN));
    set_by(&mut e, &at, "Bo's call", Some(BO));
    set_by(&mut e, &at, "Water the plant", None);
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Ann's pills"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Bo's call"]
    );
    // The owner is Ann: hers and the no-person one, never Bo's.
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), true))),
        ["Ann's pills", "Water the plant"]
    );
    // An unknown face, or the room's line: nothing of anyone's.
    assert!(listed(&mut e, &at, Some(asker(None, false))).is_empty());
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
    assert_eq!(page["not_yet_due"], 1);
}

#[test]
fn a_cancel_or_snooze_by_another_persons_id_is_not_found_and_changes_nothing() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    let ann = set_by(&mut e, &at, "Ann's pills", Some(ANN));
    let room = set_by(&mut e, &at, "Water the plant", None);
    for (id, guest) in [
        (ann, asker(Some(BO), false)),
        (ann, asker(None, false)),
        (room, asker(Some(BO), false)),
    ] {
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
    // Ann snoozes her own, and the owner cancels the no-person one.
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

/// A timer is the room's (option 1 on #4656's after-merge P2): a line with
/// no face seen, at the stove, hears and cancels the timer it set, and
/// another person's too, but still no one's reminder.
#[test]
fn every_line_hears_and_cancels_a_timer_but_never_anothers_reminder() {
    let at = now(0);
    let mut e = open(Disk::default(), &at);
    let pasta = timer_by(&mut e, &at, "Pasta", None);
    timer_by(&mut e, &at, "Tea", Some(ANN));
    let pills = set_by(&mut e, &at, "Ann's pills", Some(ANN));
    set_by(&mut e, &at, "Water the plant", None);
    let room = asker(None, false);
    assert_eq!(listed(&mut e, &at, Some(room.clone())), ["Pasta", "Tea"]);
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Pasta", "Tea"]
    );
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Pasta", "Tea", "Ann's pills"]
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
    assert_eq!(cancel(&mut e, pills).unwrap_err().code, "not_found");
    cancel(&mut e, pasta).unwrap();
    assert_eq!(listed(&mut e, &at, Some(room.clone())), ["Tea"]);
    // It keeps its setter: Ann's forget takes her timer with her reminder.
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
    assert_eq!(listed(&mut e, &at, None), ["Water the plant"]);
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
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(BO), false))),
        ["Bo's call"]
    );
}

#[test]
fn a_store_from_before_setters_loads_with_every_item_no_persons_and_keeps_the_setter_on_reopen() {
    let at = now(0);
    let disk = Disk::default();
    let mut e = open(disk.clone(), &at);
    // A reminder: a timer is every line's.
    set_by(&mut e, &at, "Call Maria", None);
    set_by(&mut e, &at, "Ann's pills", Some(ANN));
    drop(e);
    // The older build's store: no `person` on the item set without one.
    let stored: Value =
        serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
    assert!(stored["items"][0].get("person").is_none());
    assert_eq!(stored["items"][1]["person"], ANN);
    let mut e = open(disk, &at);
    assert_eq!(listed(&mut e, &at, Some(asker(None, true))), ["Call Maria"]);
    assert_eq!(
        listed(&mut e, &at, Some(asker(Some(ANN), false))),
        ["Ann's pills"]
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
