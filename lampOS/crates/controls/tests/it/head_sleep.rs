//! Sleep from the head (#711) is the software mute with the reason `sleep`:
//! a hand held three seconds sets it, and a later tap on the head clears it
//! through the same prepared, saved and committed clear as a fresh hold.
use lamp_controls::{mute::MuteRecord, receiver::Receiver};
use lamp_msgs::MuteReason;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const START: u64 = 3_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/ct-sleep-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("mute.json")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A receiver started at `START` that may clear, as the voice makes it
/// with `--controls-mute`, put to sleep by a hand from 4 s held to 7 s.
fn asleep() -> Receiver {
    let mut receiver = Receiver::with_hold_clear(MuteRecord::new("boot").unwrap(), START).unwrap();
    assert!(
        receiver
            .request_sleep(7_000_000, 4_000_000, 7_000_000)
            .unwrap()
    );
    receiver
}

#[test]
fn a_held_head_mutes_with_the_reason_sleep_and_a_later_tap_clears_it_once_saved() {
    let root = Root::new();
    let mut receiver = asleep();
    let record = receiver.mute_record().clone();
    assert!(record.muted && record.asleep());
    assert_eq!(record.reason, Some(MuteReason::Sleep));
    assert_eq!(record.through_us, 7_000_000);
    record.save(&root.file()).unwrap();
    // The sleep is kept across a restart and a new boot, reason and all.
    assert_eq!(
        MuteRecord::load(&root.file(), "boot", 8_000_000).unwrap(),
        record
    );
    let rebooted = MuteRecord::load(&root.file(), "next-boot", 10).unwrap();
    assert!(rebooted.asleep());
    // A tap from 9 s, let go at 9.2 s, wakes it: only once saved.
    assert!(
        receiver
            .request_wake(9_250_000, 9_000_000, 9_200_000)
            .unwrap()
    );
    assert!(receiver.clear_pending());
    let prepared = receiver.prepare_clear(9_260_000).unwrap();
    assert!(receiver.mute_record().asleep());
    prepared.record().save(&root.file()).unwrap();
    receiver.commit_clear(prepared).unwrap();
    let woken = receiver.mute_record();
    assert!(!woken.muted && woken.reason.is_none());
    assert_eq!(
        &MuteRecord::load(&root.file(), "boot", 9_300_000).unwrap(),
        woken
    );
    // An awake record is written as it always was: no reason in it.
    assert!(!fs::read_to_string(root.file()).unwrap().contains("reason"));
}

#[test]
fn sleep_never_takes_over_a_privacy_mute_and_a_tap_never_clears_one() {
    let mut record = MuteRecord::new("boot").unwrap();
    assert!(record.request_mute(1_000_000));
    let mut receiver = Receiver::with_hold_clear(record, START).unwrap();
    assert!(
        !receiver
            .request_sleep(7_000_000, 4_000_000, 7_000_000)
            .unwrap()
    );
    assert!(receiver.mute_record().muted && receiver.mute_record().reason.is_none());
    assert!(
        !receiver
            .request_wake(9_250_000, 9_000_000, 9_200_000)
            .unwrap()
    );
    assert!(!receiver.clear_pending());
}

#[test]
fn a_hold_of_the_button_makes_a_sleep_a_privacy_mute() {
    let mut record = MuteRecord::new("boot").unwrap();
    assert!(record.request_sleep(7_000_000));
    assert!(record.asleep());
    // Already muted, asleep or not: a second sleep changes nothing.
    assert!(!record.request_sleep(8_000_000));
    assert_eq!(record.through_us, 7_000_000);
    assert!(record.request_mute(9_000_000));
    assert!(record.muted && !record.asleep() && record.reason.is_none());
    record
        .clear_after_explicit_user_request(10_000_000)
        .unwrap();
    assert!(!record.muted && record.reason.is_none());
}

#[test]
fn a_hand_begun_before_the_last_change_asks_nothing() {
    // A tap that began before the sleep was taken cannot wake it.
    let mut receiver = asleep();
    assert!(
        !receiver
            .request_wake(7_100_000, 6_900_000, 7_050_000)
            .unwrap()
    );
    assert!(!receiver.clear_pending());
    // Woken by a tap at 9 s, it cannot be sent back to sleep by a hand that
    // was already there, only by a new one.
    assert!(
        receiver
            .request_wake(9_250_000, 9_000_000, 9_200_000)
            .unwrap()
    );
    let prepared = receiver.prepare_clear(9_260_000).unwrap();
    receiver.commit_clear(prepared).unwrap();
    assert!(
        !receiver
            .request_sleep(12_000_000, 9_100_000, 12_000_000)
            .unwrap()
    );
    assert!(!receiver.mute_record().muted);
    assert!(
        receiver
            .request_sleep(13_000_000, 10_000_000, 13_000_000)
            .unwrap()
    );
    assert!(receiver.mute_record().asleep());
}

#[test]
fn an_old_tap_or_an_owner_that_may_not_clear_never_wakes() {
    // Let go 500 ms ago or more: too old to act on.
    let mut receiver = asleep();
    assert!(
        !receiver
            .request_wake(9_700_000, 9_000_000, 9_200_000)
            .unwrap()
    );
    // Released before it began, or in the future: not a tap at all.
    assert!(
        !receiver
            .request_wake(9_750_000, 9_600_000, 9_500_000)
            .unwrap()
    );
    assert!(
        !receiver
            .request_wake(9_750_000, 9_600_000, 9_800_000)
            .unwrap()
    );
    assert!(!receiver.clear_pending());
    // `Receiver::new` stays one-way: it sleeps, and nothing wakes it.
    let mut one_way = Receiver::new(MuteRecord::new("boot").unwrap()).unwrap();
    assert!(
        one_way
            .request_sleep(7_000_000, 4_000_000, 7_000_000)
            .unwrap()
    );
    assert!(
        !one_way
            .request_wake(9_250_000, 9_000_000, 9_200_000)
            .unwrap()
    );
    assert!(!one_way.clear_pending());
}

#[test]
fn a_record_without_a_reason_reads_as_before_and_an_awake_one_with_a_reason_is_refused() {
    let root = Root::new();
    fs::write(
        root.file(),
        "{\"version\":1,\"clock_id\":\"boot\",\"muted\":true,\"through_us\":100}\n",
    )
    .unwrap();
    fs::set_permissions(root.file(), fs::Permissions::from_mode(0o600)).unwrap();
    let record = MuteRecord::load(&root.file(), "boot", 1_000).unwrap();
    assert!(record.muted && record.reason.is_none());
    fs::write(
        root.file(),
        "{\"version\":1,\"clock_id\":\"boot\",\"muted\":false,\"through_us\":100,\
         \"reason\":\"sleep\"}\n",
    )
    .unwrap();
    assert!(MuteRecord::load(&root.file(), "boot", 1_000).is_err());
}
