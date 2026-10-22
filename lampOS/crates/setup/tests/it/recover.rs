//! #3020: the policy that opens setup's hotspot when a set-up lamp's saved
//! network is gone (`lamp_setup::recover`), on a clock and looks of its own.
use lamp_setup::recover::{
    Act, JOIN_WAIT_MS, LOSS_GRACE_MS, PHONE_HOLDS_MS, RETRY_EVERY_MS, Recovery, Seen, State, on,
};

const GONE: Seen = Seen {
    online: false,
    associated: false,
    busy: false,
    phones: Some(0),
};
const BACK: Seen = Seen {
    online: true,
    ..GONE
};

/// Looks every 5 s from `from_ms` to `to_ms`, the acts asked for and when.
fn looks(policy: &mut Recovery, from_ms: u64, to_ms: u64, seen: Seen) -> Vec<(u64, Act)> {
    (from_ms..=to_ms)
        .step_by(5_000)
        .filter_map(|at| policy.step(at, seen).map(|act| (at, act)))
        .collect()
}

#[test]
fn the_network_gone_90_s_opens_the_hotspot_once() {
    let mut policy = Recovery::default();
    assert_eq!(policy.step(0, BACK), None);
    let acts = looks(&mut policy, 5_000, 200_000, GONE);
    // Lost at 5 s, open at 95 s, and not again before the 2 min retry.
    assert_eq!(acts[0], (5_000 + LOSS_GRACE_MS, Act::OpenHotspot));
    assert!(policy.recovering());
    assert_eq!(acts.len(), 1, "{acts:?}");
}

#[test]
fn the_network_back_at_80_s_opens_nothing() {
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 80_000, GONE).is_empty());
    assert_eq!(policy.step(85_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
    // Gone again: its 90 s start over from there.
    assert!(looks(&mut policy, 90_000, 175_000, GONE).is_empty());
    assert_eq!(policy.step(180_000, GONE), Some(Act::OpenHotspot));
}

#[test]
fn the_network_back_while_the_hotspot_is_open_is_rejoined_within_2_min() {
    let mut policy = Recovery::default();
    let opened = looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert_eq!(opened, [(LOSS_GRACE_MS, Act::OpenHotspot)]);
    // The router comes back at once; the hotspot has no phone on it: the
    // saved network is tried at the retry, and joined.
    let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
    let acts = looks(&mut policy, LOSS_GRACE_MS + 5_000, retry, GONE);
    assert_eq!(acts, [(retry, Act::Rejoin)]);
    assert_eq!(policy.step(retry + 10_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
    assert!(!policy.recovering());
}

#[test]
fn a_try_that_does_not_join_in_45_s_opens_the_hotspot_again() {
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
    assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
    assert_eq!(policy.step(retry + JOIN_WAIT_MS - 5_000, GONE), None);
    assert_eq!(
        policy.step(retry + JOIN_WAIT_MS, GONE),
        Some(Act::OpenHotspot)
    );
    assert!(policy.recovering());
}

/// Reviewer-two on #3024: a try of the saved network that has joined its
/// radio and waits for an address (slow DHCP), or one a Wi-Fi change has
/// taken, is never cut back to the hotspot, at 45 s or after. Once that
/// clears with no address, the whole 90 s run again before the hotspot;
/// and an address in the meantime is online.
#[test]
fn a_try_that_is_only_slow_or_busy_is_not_cut_and_then_counts_as_lost() {
    let slow = Seen {
        associated: true,
        ..GONE
    };
    let busy = Seen { busy: true, ..GONE };
    for seen in [slow, busy] {
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
        assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
        // Just before the deadline it still waits, then the guard holds.
        assert_eq!(policy.step(retry + JOIN_WAIT_MS - 5_000, GONE), None);
        assert!(
            looks(&mut policy, retry + JOIN_WAIT_MS, retry + 300_000, seen).is_empty(),
            "{seen:?}"
        );
        assert!(!policy.recovering(), "{seen:?}");
        // Cleared, no address: 90 s from the last look slow or busy.
        let cleared = retry + 300_000;
        assert!(
            looks(
                &mut policy,
                cleared + 5_000,
                cleared + LOSS_GRACE_MS - 5_000,
                GONE
            )
            .is_empty()
        );
        assert_eq!(
            policy.step(cleared + LOSS_GRACE_MS, GONE),
            Some(Act::OpenHotspot)
        );

        // The same, an address while it waits: online.
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
        looks(&mut policy, retry + 5_000, retry + 100_000, seen);
        assert_eq!(policy.step(retry + 105_000, BACK), None);
        assert_eq!(policy.state(), State::Online);
    }
}

#[test]
fn a_lamp_only_slow_to_get_its_address_never_opens_it() {
    // Joined to a known network's radio (the saved one, or another it
    // knows), its address not there yet, for 10 minutes.
    let slow = Seen {
        associated: true,
        ..GONE
    };
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, slow).is_empty());
    assert!(!policy.recovering());
}

#[test]
fn another_known_network_taken_is_online_and_opens_nothing() {
    // wpa_supplicant takes another network the saved file names: an
    // address, so online.
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, BACK).is_empty());
    assert_eq!(policy.state(), State::Online);
}

#[test]
fn setup_or_a_wifi_change_running_holds_it() {
    let busy = Seen { busy: true, ..GONE };
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, busy).is_empty());
    // Done, still gone: a whole 90 s from its last look busy (600 s).
    assert!(looks(&mut policy, 605_000, 685_000, GONE).is_empty());
    assert_eq!(policy.step(690_000, GONE), Some(Act::OpenHotspot));
}

#[test]
fn a_setup_run_is_never_cut_by_a_retry() {
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    let setting_up = Seen {
        busy: true,
        phones: Some(1),
        ..GONE
    };
    assert!(looks(&mut policy, LOSS_GRACE_MS + 5_000, 3_000_000, setting_up).is_empty());
    assert!(policy.recovering());
    // The run over, nobody on: the retry, long due.
    assert_eq!(policy.step(3_005_000, GONE), Some(Act::Rejoin));
}

/// El's P2 on #3024: a phone that joined the open hotspot by itself (it
/// remembers it from setup) holds the retry 10 min past due, not all night.
#[test]
fn a_phone_holds_a_retry_10_min_at_most() {
    for phones in [Some(1), None] {
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        let due = LOSS_GRACE_MS + RETRY_EVERY_MS;
        let phone = Seen { phones, ..GONE };
        let acts = looks(&mut policy, LOSS_GRACE_MS + 5_000, due + 900_000, phone);
        assert_eq!(acts[0], (due + PHONE_HOLDS_MS, Act::Rejoin), "{phones:?}");
    }
}

#[test]
fn a_changed_password_still_opens_it() {
    // The saved network in reach but never joining (a wrong key): not
    // associated, so gone, and the hotspot stays up between retries.
    let mut policy = Recovery::default();
    let acts = looks(&mut policy, 0, LOSS_GRACE_MS + 60_000, GONE);
    assert_eq!(acts, [(LOSS_GRACE_MS, Act::OpenHotspot)]);
}

#[test]
fn a_new_network_set_from_the_hotspot_ends_recovery() {
    // The app sets the new network through setup's page, which joins it:
    // online, and recovery is over.
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert_eq!(policy.step(LOSS_GRACE_MS + 30_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
}

#[test]
fn the_switch_is_on_for_on_only() {
    assert!(on(Some("on")));
    for value in [None, Some(""), Some("On"), Some("1"), Some("off")] {
        assert!(!on(value), "{value:?}");
    }
}
