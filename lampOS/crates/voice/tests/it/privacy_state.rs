use lamp_turn_owner::MonoTime;
use lamp_voice::privacy::{PRIVACY_FRESH_US, PrivacyGate, PrivacyState, REOPEN_STABLE_US};

fn t(us: u64) -> MonoTime {
    MonoTime::from_micros(us)
}

#[test]
fn only_a_fresh_ordered_switch_read_is_an_explicit_mute() {
    let mut gate = PrivacyGate::default();
    assert_eq!(gate.state(t(1)), PrivacyState::Unavailable);
    gate.observe(1, t(10_000), t(10_001), true);
    assert_eq!(gate.state(t(10_001)), PrivacyState::Muted);
    assert_eq!(
        gate.state(t(10_000 + PRIVACY_FRESH_US)),
        PrivacyState::Unavailable
    );
    gate.observe(2, t(70_000), t(70_001), true);
    assert_eq!(gate.state(t(70_001)), PrivacyState::Muted);
    gate.observe(1, t(60_000), t(70_002), true);
    assert_eq!(gate.state(t(70_002)), PrivacyState::Unavailable);
    // Bad ordering must not roll the high-water mark backwards.
    gate.observe(2, t(70_000), t(70_003), true);
    assert_eq!(gate.state(t(70_003)), PrivacyState::Unavailable);
    gate.observe(3, t(80_000), t(80_001), true);
    assert_eq!(gate.state(t(80_001)), PrivacyState::Muted);
}

#[test]
fn unmute_stabilizes_without_conflating_it_with_mute_or_a_fault() {
    let mut gate = PrivacyGate::default();
    gate.observe(1, t(10_000), t(10_000), true);
    for (index, us) in (20_000..=80_000).step_by(10_000).enumerate() {
        let allowed = gate.observe(index as u64 + 2, t(us), t(us), false);
        let expected = if us - 20_000 >= REOPEN_STABLE_US {
            PrivacyState::Allowed
        } else {
            PrivacyState::Stabilizing
        };
        assert_eq!(gate.state(t(us)), expected);
        assert_eq!(allowed, expected == PrivacyState::Allowed);
    }
    gate.observe(9, t(90_000), t(90_000), true);
    assert_eq!(gate.state(t(90_000)), PrivacyState::Muted);
    assert!(!gate.allowed(t(90_000)));
}

#[test]
fn an_observation_gap_restarts_unmute_stability() {
    let mut gate = PrivacyGate::default();
    gate.observe(1, t(10_000), t(10_000), false);
    gate.observe(2, t(70_000), t(70_000), false);
    assert_eq!(gate.state(t(70_000)), PrivacyState::Stabilizing);
    for (i, us) in (80_000..=130_000).step_by(10_000).enumerate() {
        gate.observe(i as u64 + 3, t(us), t(us), false);
    }
    assert_eq!(gate.state(t(130_000)), PrivacyState::Allowed);
}

#[test]
fn invalid_reads_and_explicit_close_cannot_be_reported_as_user_mute() {
    for (acquired, now) in [(10_000, 9_999), (10_000, 60_000)] {
        let mut gate = PrivacyGate::default();
        gate.observe(1, t(acquired), t(now), true);
        assert_eq!(gate.state(t(now)), PrivacyState::Unavailable);
    }
    let mut gate = PrivacyGate::default();
    gate.observe(1, t(10_000), t(10_001), true);
    gate.close();
    assert_eq!(gate.state(t(10_002)), PrivacyState::Unavailable);
    assert_eq!(gate.state(t(9_999)), PrivacyState::Unavailable);
}
