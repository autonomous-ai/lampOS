use lamp_mood::engine::{
    AIR_QUALIFY_US, DIRECT_ATTENTION_US, ENERGY_HALF_LIFE_US, Engine, Event, EventKind,
    FACE_ATTENTION_US, FaceTarget, MAX_EVENT_LEASE_US, MINUTE_US, NUDGE_FADE_US, QUIET_BEGIN_US,
    QUIET_FULL_US, Reject, SECOND_US, SUDDEN_COOLDOWN_US, WARMTH_COOLDOWN_US, WARMTH_HALF_LIFE_US,
};
use lamp_msgs::{
    AttentionTarget, Familiarity, MOOD_STATE_PERIOD_US, MOOD_STATE_TTL_US, MoodState, mood_topics,
};

const DAY: Option<u16> = Some(12 * 60);
const NIGHT: Option<u16> = Some(23 * 60);
const BOOT: &str = "0123456789abcdef";
const NEXT_BOOT: &str = "fedcba9876543210";

fn engine(minute: Option<u16>) -> Engine {
    Engine::new("host-boot".into(), 0, minute).unwrap()
}

fn leased(observed_us: u64, duration_us: u64, kind: EventKind) -> Event {
    Event {
        observed_us,
        valid_until_us: observed_us + duration_us,
        kind,
    }
}

fn event(at: u64, kind: EventKind) -> Event {
    leased(at, SECOND_US, kind)
}

fn send(engine: &mut Engine, at: u64, kind: EventKind) {
    engine.apply(at, event(at, kind)).unwrap();
}

fn state(engine: &Engine) -> MoodState {
    engine.snapshot(engine.now_us(), 1, "mood-session").unwrap()
}

fn face(boot: &str, id: &str) -> FaceTarget {
    FaceTarget {
        publisher_boot: boot.into(),
        id: id.into(),
    }
}

fn close(got: f32, expected: f32) {
    assert!(
        (got - expected).abs() <= 0.000_002,
        "got {got}, expected {expected}"
    );
}

fn same_values(left: &Engine, right: &Engine) {
    let (left, right) = (state(left), state(right));
    close(left.awake, right.awake);
    close(left.energy, right.energy);
    close(left.warmth, right.warmth);
    assert_eq!(left.asleep, right.asleep);
    assert_eq!(left.attention, right.attention);
}

#[test]
fn startup_has_explicit_civil_time_boundaries_and_no_personal_state() {
    for (minute, awake, asleep) in [
        (None, 0.40, false),
        (Some(0), 0.0, true),
        (Some(419), 0.0, true),
        (Some(420), 0.70, false),
        (Some(1_139), 0.70, false),
        (Some(1_140), 0.40, false),
        (Some(1_319), 0.40, false),
        (Some(1_320), 0.0, true),
        (Some(1_439), 0.0, true),
    ] {
        let value = state(&engine(minute));
        close(value.awake, awake);
        close(value.energy, 0.30);
        close(value.warmth, 0.60);
        assert_eq!(value.asleep, asleep);
        assert_eq!(value.attention, None);
        assert_eq!(value.familiarity, Familiarity::Unknown);
    }
}

#[test]
fn invalid_initial_identity_and_civil_minute_are_rejected() {
    for id in ["", "white space", "new\nline"] {
        assert_eq!(
            Engine::new(id.into(), 0, DAY).unwrap_err(),
            Reject::InvalidClock
        );
    }
    assert_eq!(
        Engine::new("x".repeat(129), 0, DAY).unwrap_err(),
        Reject::InvalidClock
    );
    assert_eq!(
        Engine::new("host".into(), 0, Some(1_440)).unwrap_err(),
        Reject::InvalidLocalMinute
    );
}

#[test]
fn snapshots_are_pure_and_require_an_advanced_clock_and_valid_identity() {
    let mut mood = engine(DAY);
    send(&mut mood, 100, EventKind::Name);
    let before = mood.clone();
    assert_eq!(mood.snapshot(99, 1, "run"), Err(Reject::SnapshotClock));
    assert_eq!(mood.snapshot(101, 1, "run"), Err(Reject::SnapshotClock));
    assert_eq!(mood.snapshot(100, 0, "run"), Err(Reject::InvalidSnapshot));
    assert_eq!(mood.snapshot(100, 1, ""), Err(Reject::InvalidSnapshot));
    assert_eq!(mood, before);
}

#[test]
fn observation_leases_are_half_open_bounded_and_not_refreshed_by_receipt() {
    let mut mood = engine(DAY);
    let before = mood.clone();
    for (now, value, why) in [
        (0, event(1, EventKind::Name), Reject::FutureEvent),
        (100, leased(100, 0, EventKind::Name), Reject::InvalidLease),
        (
            100,
            leased(100, MAX_EVENT_LEASE_US + 1, EventKind::Name),
            Reject::InvalidLease,
        ),
        (
            SECOND_US + 100,
            event(100, EventKind::Name),
            Reject::ExpiredEvent,
        ),
    ] {
        assert_eq!(mood.apply(now, value), Err(why));
        assert_eq!(mood, before);
    }
    assert!(
        mood.apply(SECOND_US + 99, event(100, EventKind::Name))
            .is_ok()
    );
}

#[test]
fn malformed_face_identity_is_rejected_without_retaining_it() {
    let mut mood = engine(DAY);
    let before = mood.clone();
    for face in [
        face("", "f1"),
        face("ABCDEF0123456789", "f1"),
        face(BOOT, ""),
        face(BOOT, &"x".repeat(129)),
        face(BOOT, "f\n1"),
    ] {
        assert_eq!(
            mood.apply(1, event(1, EventKind::Faces(Some(face)))),
            Err(Reject::InvalidFace)
        );
        assert_eq!(mood, before);
    }
}

#[test]
fn backward_monotonic_time_or_invalid_clock_sample_never_mutates_state() {
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Touch { kind: true });
    let before = mood.clone();
    assert_eq!(
        mood.advance(SECOND_US - 1, DAY),
        Err(Reject::ClockRegression)
    );
    assert_eq!(
        mood.apply(SECOND_US - 1, event(SECOND_US - 1, EventKind::Name)),
        Err(Reject::ClockRegression)
    );
    assert_eq!(
        mood.advance(SECOND_US + 1, Some(1_440)),
        Err(Reject::InvalidLocalMinute)
    );
    assert_eq!(mood, before);
}

#[test]
fn equal_or_older_observation_on_one_stream_is_not_another_event() {
    let mut mood = engine(DAY);
    send(&mut mood, 100, EventKind::Touch { kind: true });
    let before = mood.clone();
    for observed in [99, 100] {
        assert_eq!(
            mood.apply(101, event(observed, EventKind::Touch { kind: true })),
            Err(Reject::OutOfOrder)
        );
        assert_eq!(mood, before);
    }
    send(&mut mood, 101, EventKind::Touch { kind: false });
    assert_eq!(state(&mood).attention, Some(AttentionTarget::Touch));
}

#[test]
fn a_sudden_sound_has_bounded_energy_and_an_exact_half_life() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::SuddenSound);
    close(state(&mood).energy, 0.55);
    assert_eq!(state(&mood).attention, Some(AttentionTarget::Sound));
    mood.advance(1 + ENERGY_HALF_LIFE_US, DAY).unwrap();
    close(state(&mood).energy, 0.425);
    assert_eq!(state(&mood).attention, None);
    mood.advance(1 + 2 * ENERGY_HALF_LIFE_US, DAY).unwrap();
    close(state(&mood).energy, 0.3625);
}

#[test]
fn repeated_noise_cannot_renew_a_startle_or_keep_the_lamp_awake() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::SuddenSound);
    let mut expected = mood.clone();
    let inside = SUDDEN_COOLDOWN_US;
    expected.advance(inside, DAY).unwrap();
    send(&mut mood, inside, EventKind::SuddenSound);
    same_values(&mood, &expected);
    send(&mut mood, 1 + SUDDEN_COOLDOWN_US, EventKind::SuddenSound);
    assert!(state(&mood).energy > 0.55);
    assert!(state(&mood).energy <= 0.70);
    mood.advance(QUIET_FULL_US, DAY).unwrap();
    assert!(state(&mood).asleep);
}

#[test]
fn transport_age_is_applied_to_energy_and_warmth_impulses() {
    for kind in [EventKind::SuddenSound, EventKind::Thanks] {
        let mut timely = engine(DAY);
        let mut delayed = engine(DAY);
        send(&mut timely, 1, kind.clone());
        timely.advance(10 * SECOND_US + 1, DAY).unwrap();
        delayed
            .apply(10 * SECOND_US + 1, leased(1, MAX_EVENT_LEASE_US, kind))
            .unwrap();
        close(state(&timely).energy, state(&delayed).energy);
        close(state(&timely).warmth, state(&delayed).warmth);
        assert_eq!(state(&delayed).attention, None);
    }
}

#[test]
fn an_attending_tap_wakes_but_only_kind_touch_warms() {
    let mut tap = engine(NIGHT);
    send(&mut tap, 1, EventKind::Touch { kind: false });
    assert!(!state(&tap).asleep);
    close(state(&tap).awake, 0.85);
    close(state(&tap).warmth, 0.60);
    let mut kind = engine(NIGHT);
    send(&mut kind, 1, EventKind::Touch { kind: true });
    close(state(&kind).warmth, 0.65);
    assert_eq!(state(&kind).attention, Some(AttentionTarget::Touch));
}

#[test]
fn kind_touch_thanks_and_laughter_share_a_cooldown_and_a_warmth_cap() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Thanks);
    close(state(&mood).warmth, 0.64);
    let mut expected = mood.clone();
    expected.advance(2, DAY).unwrap();
    send(&mut mood, 2, EventKind::Laughter);
    close(state(&mood).warmth, state(&expected).warmth);
    send(&mut mood, 1 + WARMTH_COOLDOWN_US, EventKind::Laughter);
    assert!(state(&mood).warmth > 0.669);
    for step in 2..40 {
        send(
            &mut mood,
            1 + step * WARMTH_COOLDOWN_US,
            EventKind::Touch { kind: true },
        );
        assert!(state(&mood).warmth <= 0.85);
    }
    close(state(&mood).warmth, 0.85);
}

#[test]
fn ignored_warmth_returns_to_neutral_and_never_becomes_resentment() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Thanks);
    mood.advance(1 + WARMTH_HALF_LIFE_US, DAY).unwrap();
    close(state(&mood).warmth, 0.62);
    mood.advance(u64::MAX, DAY).unwrap();
    close(state(&mood).warmth, 0.60);
    close(state(&mood).energy, 0.30);
    assert!(state(&mood).asleep);
}

#[test]
fn company_sources_are_presence_leases_without_additive_personality_gain() {
    let mut one = engine(DAY);
    let mut three = engine(DAY);
    one.apply(1, leased(1, MAX_EVENT_LEASE_US, EventKind::Voice(true)))
        .unwrap();
    for kind in [
        EventKind::Voice(true),
        EventKind::Heard,
        EventKind::Faces(Some(face(BOOT, "f1"))),
    ] {
        three.apply(1, leased(1, MAX_EVENT_LEASE_US, kind)).unwrap();
    }
    one.advance(14 * SECOND_US, DAY).unwrap();
    three.advance(14 * SECOND_US, DAY).unwrap();
    close(state(&one).awake, state(&three).awake);
    close(state(&three).energy, 0.30);
    close(state(&three).warmth, 0.60);
}

#[test]
fn one_company_source_becoming_inactive_does_not_clear_the_others() {
    let mut both = engine(DAY);
    let mut voice = engine(DAY);
    for mood in [&mut both, &mut voice] {
        mood.apply(1, leased(1, MAX_EVENT_LEASE_US, EventKind::Voice(true)))
            .unwrap();
    }
    both.apply(1, leased(1, MAX_EVENT_LEASE_US, EventKind::Heard))
        .unwrap();
    for mood in [&mut both, &mut voice] {
        send(mood, SECOND_US, EventKind::Voice(false));
        mood.advance(14 * SECOND_US, DAY).unwrap();
    }
    assert!(state(&both).awake > state(&voice).awake);
}

#[test]
fn a_missing_company_source_expires_and_does_not_prevent_sleep() {
    let mut mood = engine(DAY);
    mood.apply(1, leased(1, MAX_EVENT_LEASE_US, EventKind::Voice(true)))
        .unwrap();
    mood.advance(1 + QUIET_FULL_US, DAY).unwrap();
    assert!(state(&mood).asleep);
}

#[test]
fn continuous_company_prevents_auto_sleep_without_unbounded_awake_boost() {
    let mut mood = engine(DAY);
    for step in 0..=720 {
        let at = 1 + step * 5 * SECOND_US;
        mood.apply(at, leased(at, MAX_EVENT_LEASE_US, EventKind::Voice(true)))
            .unwrap();
    }
    let value = state(&mood);
    assert!(!value.asleep);
    close(value.awake, 0.85);
    close(value.energy, 0.30);
    close(value.warmth, 0.60);
}

#[test]
fn ordinary_company_and_semantics_do_not_wake_a_sleeping_lamp() {
    let mut mood = engine(NIGHT);
    for (index, kind) in [
        EventKind::SuddenSound,
        EventKind::Thanks,
        EventKind::Laughter,
        EventKind::Voice(true),
        EventKind::Heard,
        EventKind::Faces(Some(face(BOOT, "f1"))),
        EventKind::Air(Some(2_000)),
    ]
    .into_iter()
    .enumerate()
    {
        send(&mut mood, (index + 1) as u64, kind);
        let value = state(&mood);
        assert!(value.asleep);
        close(value.awake, 0.0);
        close(value.energy, 0.30);
        close(value.warmth, 0.60);
        assert_eq!(value.attention, None);
    }
    mood.advance(8, DAY).unwrap();
    mood.advance(24 * 60 * MINUTE_US, DAY).unwrap();
    assert!(state(&mood).asleep);
}

#[test]
fn only_name_touch_or_a_user_reminder_clear_sleep() {
    for kind in [
        EventKind::Name,
        EventKind::Touch { kind: false },
        EventKind::UserReminder,
    ] {
        let mut mood = engine(NIGHT);
        send(&mut mood, 1, kind.clone());
        let value = state(&mood);
        assert!(!value.asleep);
        assert!(value.awake >= 0.85);
        assert_eq!(
            value.attention,
            match kind {
                EventKind::Name => Some(AttentionTarget::Sound),
                EventKind::Touch { .. } => Some(AttentionTarget::Touch),
                _ => None,
            }
        );
    }
}

#[test]
fn a_reminder_does_not_invent_a_direction_or_keep_an_old_target() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Touch { kind: false });
    send(&mut mood, 2, EventKind::UserReminder);
    assert_eq!(state(&mood).attention, None);
    close(state(&mood).energy, 0.30);
    close(state(&mood).warmth, 0.60);
}

#[test]
fn queued_pre_sleep_observations_cannot_wake_or_affect_a_new_awake_period() {
    let mut mood = engine(DAY);
    send(&mut mood, 10 * SECOND_US, EventKind::Sleep);
    for kind in [
        EventKind::Name,
        EventKind::Touch { kind: true },
        EventKind::UserReminder,
        EventKind::SuddenSound,
        EventKind::Heard,
    ] {
        let before = mood.clone();
        assert_eq!(
            mood.apply(
                11 * SECOND_US,
                leased(9 * SECOND_US, MAX_EVENT_LEASE_US, kind)
            ),
            Err(Reject::BeforeSleep)
        );
        assert_eq!(mood, before);
    }
    assert_eq!(
        mood.apply(
            11 * SECOND_US,
            leased(10 * SECOND_US, MAX_EVENT_LEASE_US, EventKind::Name)
        ),
        Err(Reject::BeforeSleep)
    );
    send(&mut mood, 11 * SECOND_US, EventKind::Name);
    assert_eq!(
        mood.apply(
            11 * SECOND_US + 1,
            leased(9 * SECOND_US, MAX_EVENT_LEASE_US, EventKind::Thanks)
        ),
        Err(Reject::BeforeSleep)
    );
    assert!(!state(&mood).asleep);
}

#[test]
fn automatic_sleep_also_fences_a_delayed_wake_observation_atomically() {
    let mut mood = engine(DAY);
    let before = mood.clone();
    assert_eq!(
        mood.apply(
            QUIET_FULL_US + SECOND_US,
            leased(QUIET_FULL_US - 1, MAX_EVENT_LEASE_US, EventKind::Name)
        ),
        Err(Reject::BeforeSleep)
    );
    assert_eq!(mood, before);
    mood.advance(QUIET_FULL_US + SECOND_US, DAY).unwrap();
    assert!(state(&mood).asleep);
    mood.apply(
        QUIET_FULL_US + SECOND_US,
        leased(QUIET_FULL_US + 1, MAX_EVENT_LEASE_US, EventKind::Name),
    )
    .unwrap();
    assert!(!state(&mood).asleep);
}

#[test]
fn day_and_night_sleep_use_exact_inactivity_boundaries_without_tick_dwell() {
    let mut day = engine(DAY);
    day.advance(QUIET_FULL_US - 1, DAY).unwrap();
    assert!(!state(&day).asleep);
    day.advance(QUIET_FULL_US, DAY).unwrap();
    assert!(state(&day).asleep);
    let mut night = engine(NIGHT);
    send(&mut night, 1, EventKind::Name);
    night.advance(QUIET_BEGIN_US, NIGHT).unwrap();
    assert!(!state(&night).asleep);
    night.advance(QUIET_BEGIN_US + 1, NIGHT).unwrap();
    assert!(state(&night).asleep);
}

#[test]
fn face_arrival_attention_is_not_extended_forever_by_frame_heartbeats() {
    let mut mood = engine(DAY);
    for frame in 0..=8 {
        let at = 1 + frame * SECOND_US / 2;
        send(&mut mood, at, EventKind::Faces(Some(face(BOOT, "f1"))));
        if frame >= 4 {
            assert_eq!(state(&mood).attention, None);
        } else {
            assert!(matches!(
                state(&mood).attention,
                Some(AttentionTarget::Face { .. })
            ));
        }
    }
    send(&mut mood, 5 * SECOND_US, EventKind::Faces(None));
    send(
        &mut mood,
        5 * SECOND_US + 1,
        EventKind::Faces(Some(face(BOOT, "f1"))),
    );
    assert!(matches!(
        state(&mood).attention,
        Some(AttentionTarget::Face { .. })
    ));
}

#[test]
fn name_uses_only_a_current_scoped_face_and_a_stale_face_is_cleared() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Faces(Some(face(BOOT, "f1"))));
    send(&mut mood, 2, EventKind::Name);
    assert_eq!(
        state(&mood).attention,
        Some(AttentionTarget::Face {
            publisher_boot: BOOT.into(),
            id: "f1".into()
        })
    );
    mood.advance(SECOND_US + 1, DAY).unwrap();
    assert_eq!(state(&mood).attention, None);
    send(&mut mood, SECOND_US + 2, EventKind::Name);
    assert_eq!(state(&mood).attention, Some(AttentionTarget::Sound));
}

#[test]
fn new_face_publisher_identity_never_inherits_the_previous_direct_focus() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Faces(Some(face(BOOT, "f1"))));
    send(&mut mood, 2, EventKind::Name);
    send(&mut mood, 3, EventKind::Faces(Some(face(NEXT_BOOT, "f1"))));
    assert_eq!(
        state(&mood).attention,
        Some(AttentionTarget::Face {
            publisher_boot: NEXT_BOOT.into(),
            id: "f1".into()
        })
    );
    for frame in 1..=5 {
        send(
            &mut mood,
            3 + frame * SECOND_US / 2,
            EventKind::Faces(Some(face(NEXT_BOOT, "f1"))),
        );
    }
    // The replacement only acquired arrival priority and its two-second
    // lease, not the old name's five-second direct-attention lease.
    assert_eq!(state(&mood).attention, None);
}

#[test]
fn direct_attention_wins_over_company_and_noise_then_expires_without_resurrection() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Faces(Some(face(BOOT, "f1"))));
    send(&mut mood, 2, EventKind::Touch { kind: false });
    send(&mut mood, 3, EventKind::SuddenSound);
    assert_eq!(state(&mood).attention, Some(AttentionTarget::Touch));
    for frame in 1..=12 {
        send(
            &mut mood,
            3 + frame * SECOND_US / 2,
            EventKind::Faces(Some(face(BOOT, "f1"))),
        );
        if mood.now_us() >= 2 + DIRECT_ATTENTION_US {
            assert_eq!(state(&mood).attention, None);
        }
    }
}

#[test]
fn face_attention_wins_over_incidental_sound_but_not_a_direct_name() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Faces(Some(face(BOOT, "f1"))));
    send(&mut mood, 2, EventKind::SuddenSound);
    assert!(matches!(
        state(&mood).attention,
        Some(AttentionTarget::Face { .. })
    ));
    send(&mut mood, 3, EventKind::Name);
    for frame in 1..=6 {
        send(
            &mut mood,
            3 + frame * SECOND_US / 2,
            EventKind::Faces(Some(face(BOOT, "f1"))),
        );
    }
    assert!(mood.now_us() > FACE_ATTENTION_US);
    assert!(matches!(
        state(&mood).attention,
        Some(AttentionTarget::Face { .. })
    ));
}

fn fresh_air_until(mood: &mut Engine, from_us: u64, through_us: u64, co2: u16) {
    for at in (from_us..=through_us).step_by((5 * SECOND_US) as usize) {
        mood.apply(
            at,
            leased(at, MAX_EVENT_LEASE_US, EventKind::Air(Some(co2))),
        )
        .unwrap();
    }
}

#[test]
fn stuffy_air_needs_ten_fresh_minutes_then_approaches_the_bounded_reduction() {
    let mut mood = engine(DAY);
    fresh_air_until(&mut mood, 1, 1 + AIR_QUALIFY_US, 1_200);
    close(state(&mood).energy, 0.30);
    fresh_air_until(
        &mut mood,
        1 + AIR_QUALIFY_US + 5 * SECOND_US,
        1 + AIR_QUALIFY_US + 5 * MINUTE_US,
        1_200,
    );
    close(state(&mood).energy, 0.26);
    assert_eq!(state(&mood).attention, None);
    fresh_air_until(
        &mut mood,
        1 + AIR_QUALIFY_US + 5 * MINUTE_US + 5 * SECOND_US,
        1 + 45 * MINUTE_US,
        60_000,
    );
    assert!(state(&mood).energy >= 0.22);
    assert!(state(&mood).energy < 0.221);
}

#[test]
fn a_stale_air_gap_or_explicit_unknown_breaks_the_qualification_streak() {
    let mut mood = engine(DAY);
    fresh_air_until(&mut mood, 1, 1 + 9 * MINUTE_US, 1_900);
    let after_gap = 1 + 10 * MINUTE_US;
    fresh_air_until(&mut mood, after_gap, after_gap + 9 * MINUTE_US, 1_900);
    close(state(&mood).energy, 0.30);
    let unknown = after_gap + 9 * MINUTE_US + SECOND_US;
    send(&mut mood, unknown, EventKind::Air(None));
    fresh_air_until(
        &mut mood,
        unknown + SECOND_US,
        unknown + SECOND_US + 9 * MINUTE_US,
        1_900,
    );
    close(state(&mood).energy, 0.30);
}

#[test]
fn fresh_air_below_threshold_does_not_qualify_and_stale_reduction_fades() {
    let mut low = engine(DAY);
    fresh_air_until(&mut low, 1, 1 + 15 * MINUTE_US, 1_199);
    close(state(&low).energy, 0.30);
    let mut high = engine(DAY);
    fresh_air_until(&mut high, 1, 1 + 15 * MINUTE_US, 1_900);
    let reduced = state(&high).energy;
    high.advance(1 + 15 * MINUTE_US + MAX_EVENT_LEASE_US, DAY)
        .unwrap();
    let at_expiry = state(&high).energy;
    assert!(at_expiry < reduced);
    high.advance(1 + 20 * MINUTE_US + MAX_EVENT_LEASE_US, DAY)
        .unwrap();
    close(state(&high).energy, 0.30 - (0.30 - at_expiry) / 2.0);
}

#[test]
fn air_can_change_style_while_asleep_but_cannot_wake_or_claim_attention() {
    let mut mood = engine(NIGHT);
    fresh_air_until(&mut mood, 1, 1 + 15 * MINUTE_US, 1_900);
    let value = state(&mood);
    assert!(value.asleep);
    close(value.awake, 0.0);
    close(value.energy, 0.26);
    assert_eq!(value.attention, None);
}

#[test]
fn awake_quiet_ramp_and_event_decay_are_tick_rate_independent() {
    let end = 45 * MINUTE_US;
    let mut coarse = engine(DAY);
    let mut fine = engine(DAY);
    for mood in [&mut coarse, &mut fine] {
        send(mood, 1, EventKind::SuddenSound);
        send(mood, 2, EventKind::Thanks);
    }
    coarse.advance(end, DAY).unwrap();
    for now in (SECOND_US..end).step_by(SECOND_US as usize) {
        fine.advance(now, DAY).unwrap();
    }
    fine.advance(end, DAY).unwrap();
    same_values(&coarse, &fine);
    assert!(!state(&coarse).asleep);
}

#[test]
fn source_lease_expiry_inside_a_long_step_matches_frequent_steps() {
    let mut coarse = engine(DAY);
    let mut fine = engine(DAY);
    for mood in [&mut coarse, &mut fine] {
        mood.apply(
            1,
            leased(
                1,
                MAX_EVENT_LEASE_US,
                EventKind::Faces(Some(face(BOOT, "f1"))),
            ),
        )
        .unwrap();
        send(mood, 2, EventKind::Name);
    }
    coarse.advance(45 * MINUTE_US, DAY).unwrap();
    for now in (SECOND_US..=45 * MINUTE_US).step_by(SECOND_US as usize) {
        fine.advance(now, DAY).unwrap();
    }
    same_values(&coarse, &fine);
}

#[test]
fn continuous_air_qualification_and_stale_decay_are_tick_rate_independent() {
    let mut coarse = engine(DAY);
    let mut fine = engine(DAY);
    for step in 0..=144 {
        let at = 1 + step * 5 * SECOND_US;
        if step > 0 {
            for second in 1..5 {
                fine.advance(at - (5 - second) * SECOND_US, DAY).unwrap();
            }
        }
        for mood in [&mut coarse, &mut fine] {
            mood.apply(
                at,
                leased(at, MAX_EVENT_LEASE_US, EventKind::Air(Some(1_900))),
            )
            .unwrap();
        }
    }
    same_values(&coarse, &fine);
    assert!(state(&coarse).energy < 0.30);
    let from = coarse.now_us();
    let end = from + 10 * MINUTE_US;
    coarse.advance(end, DAY).unwrap();
    for now in ((from + SECOND_US)..=end).step_by(SECOND_US as usize) {
        fine.advance(now, DAY).unwrap();
    }
    same_values(&coarse, &fine);
}

#[test]
fn an_evening_target_crossing_zero_remains_tick_rate_independent() {
    let evening = Some(20 * 60);
    let mut coarse = engine(evening);
    let mut fine = engine(evening);
    let end = 55 * MINUTE_US;
    coarse.advance(end, evening).unwrap();
    for now in (SECOND_US..=end).step_by(SECOND_US as usize) {
        fine.advance(now, evening).unwrap();
    }
    same_values(&coarse, &fine);
    assert!(!state(&coarse).asleep);
    assert!(state(&coarse).awake < 0.01);
}

#[test]
fn deadlines_near_the_monotonic_limit_saturate_without_extending_attention() {
    let began = u64::MAX - 10;
    let mut mood = Engine::new("host".into(), began, DAY).unwrap();
    mood.apply(
        began + 1,
        Event {
            observed_us: began + 1,
            valid_until_us: u64::MAX,
            kind: EventKind::Touch { kind: true },
        },
    )
    .unwrap();
    assert_eq!(state(&mood).attention, Some(AttentionTarget::Touch));
    mood.advance(u64::MAX, DAY).unwrap();
    assert_eq!(state(&mood).attention, None);
    assert!(state(&mood).check().is_ok());
    let before = mood.clone();
    assert_eq!(
        mood.apply(
            u64::MAX,
            Event {
                observed_us: u64::MAX,
                valid_until_us: u64::MAX,
                kind: EventKind::Name,
            },
        ),
        Err(Reject::InvalidLease)
    );
    assert_eq!(mood, before);
}

#[test]
fn sleep_entry_is_independent_of_long_tick_and_accepts_only_later_events() {
    let end = QUIET_FULL_US + SECOND_US;
    let mut coarse = engine(DAY);
    let mut fine = engine(DAY);
    coarse.advance(end, DAY).unwrap();
    for now in (SECOND_US..=end).step_by(SECOND_US as usize) {
        fine.advance(now, DAY).unwrap();
    }
    same_values(&coarse, &fine);
    for mood in [&mut coarse, &mut fine] {
        mood.apply(
            end,
            leased(QUIET_FULL_US + 1, MAX_EVENT_LEASE_US, EventKind::Name),
        )
        .unwrap();
        assert!(!state(mood).asleep);
    }
}

#[test]
fn long_jump_ends_at_neutral_with_no_catch_up_pulses() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, EventKind::Touch { kind: true });
    send(&mut mood, 2, EventKind::SuddenSound);
    mood.advance(u64::MAX, None).unwrap();
    let value = state(&mood);
    assert!(value.asleep);
    close(value.awake, 0.0);
    close(value.energy, 0.30);
    close(value.warmth, 0.60);
    assert_eq!(value.attention, None);
    assert_eq!(value.check(), Ok(()));
}

#[test]
fn wall_clock_samples_do_not_reset_activity_or_wake_sleep() {
    let mut mood = engine(NIGHT);
    for minute in [DAY, Some(19 * 60), NIGHT, None] {
        mood.advance(mood.now_us() + 1, minute).unwrap();
        assert!(state(&mood).asleep);
    }
    send(&mut mood, 5, EventKind::Name);
    mood.advance(5 + QUIET_FULL_US, None).unwrap();
    assert!(state(&mood).asleep);
}

#[test]
fn changed_reports_attention_edges_without_treating_a_face_heartbeat_as_an_edge() {
    let mut mood = engine(DAY);
    assert!(
        mood.apply(1, event(1, EventKind::Faces(Some(face(BOOT, "f1")))))
            .unwrap()
    );
    assert!(
        !mood
            .apply(2, event(2, EventKind::Faces(Some(face(BOOT, "f1")))))
            .unwrap()
    );
    assert!(mood.apply(3, event(3, EventKind::Faces(None))).unwrap());
}

#[test]
fn a_new_process_starts_neutral_without_recovering_person_or_mood_history() {
    let mut old = engine(DAY);
    send(&mut old, 1, EventKind::Touch { kind: true });
    send(&mut old, 2, EventKind::SuddenSound);
    let new = Engine::new("new-boot".into(), 1_000_000, DAY).unwrap();
    let value = state(&new);
    close(value.energy, 0.30);
    close(value.warmth, 0.60);
    assert_eq!(value.attention, None);
    assert_eq!(value.familiarity, Familiarity::Unknown);
    assert_ne!(value.stamp.clock_id, state(&old).stamp.clock_id);
}

#[test]
fn mood_output_contract_has_one_expiring_topic_and_explicit_unknown_familiarity() {
    let topics = mood_topics();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].0, "mood/state");
    let declaration = MoodState::declaration();
    assert_eq!(declaration.type_name, MoodState::TYPE);
    assert_eq!(declaration.version, 1);
    assert_eq!(declaration.kind, lamp_bus::Kind::Latest);
    assert_eq!(declaration.period_ms, Some(MOOD_STATE_PERIOD_US / 1_000));
    assert_eq!(declaration.lifespan_ms, Some(MOOD_STATE_TTL_US / 1_000));
    let original = state(&engine(DAY));
    let json = serde_json::to_value(&original).unwrap();
    assert_eq!(json["familiarity"], "unknown");
    for absent in [
        "attention",
        "text",
        "person",
        "microphone",
        "accepting_input",
        "ready",
    ] {
        assert!(json.get(absent).is_none(), "{absent}");
    }
    assert_eq!(serde_json::from_value::<MoodState>(json).unwrap(), original);
}

#[test]
fn mood_output_validation_rejects_invalid_units_or_sleep_attention() {
    let good = state(&engine(DAY));
    for invalid in [f32::NAN, f32::INFINITY, -0.001, 1.001] {
        for field in 0..3 {
            let mut bad = good.clone();
            match field {
                0 => bad.awake = invalid,
                1 => bad.energy = invalid,
                _ => bad.warmth = invalid,
            }
            assert!(bad.check().is_err());
        }
    }
    let mut bad = good.clone();
    bad.asleep = true;
    assert!(bad.check().is_err());
    bad.awake = 0.0;
    bad.attention = Some(AttentionTarget::Touch);
    assert!(bad.check().is_err());
    bad.attention = None;
    assert!(bad.check().is_ok());
}

#[test]
fn metadata_event_round_trip_carries_no_speech_or_face_history() {
    let event = event(1, EventKind::Touch { kind: true });
    let bytes = serde_json::to_vec(&event).unwrap();
    assert_eq!(serde_json::from_slice::<Event>(&bytes).unwrap(), event);
    let json = String::from_utf8(bytes).unwrap();
    assert!(!json.contains("text"));
    assert!(!json.contains("history"));
}

/// #1670: Mood holds the familiar level the eyes said only for its lease,
/// says it in its state, drops it when the eyes say nobody remembered, and
/// a change of level is a change worth saying.
#[test]
fn the_familiar_level_is_held_only_for_its_lease() {
    use lamp_msgs::FamiliarLevel;
    let mut mood = engine(DAY);
    assert_eq!(state(&mood).familiar, None);
    let at = 10 * SECOND_US;
    let changed = mood
        .apply(
            at,
            leased(
                at,
                5 * SECOND_US,
                EventKind::Familiar(Some(FamiliarLevel::Regular)),
            ),
        )
        .unwrap();
    assert!(changed);
    assert_eq!(state(&mood).familiar, Some(FamiliarLevel::Regular));
    // Gone at the end of its lease, with nothing newer.
    mood.advance(at + 5 * SECOND_US - 1, DAY).unwrap();
    assert_eq!(state(&mood).familiar, Some(FamiliarLevel::Regular));
    assert!(mood.advance(at + 5 * SECOND_US, DAY).unwrap());
    assert_eq!(state(&mood).familiar, None);
    // Said again, then nobody remembered: dropped at once.
    let again = at + 6 * SECOND_US;
    mood.apply(
        again,
        leased(
            again,
            5 * SECOND_US,
            EventKind::Familiar(Some(FamiliarLevel::Close)),
        ),
    )
    .unwrap();
    assert_eq!(state(&mood).familiar, Some(FamiliarLevel::Close));
    let gone = again + SECOND_US;
    assert!(
        mood.apply(gone, leased(gone, 5 * SECOND_US, EventKind::Familiar(None)))
            .unwrap()
    );
    assert_eq!(state(&mood).familiar, None);
}

/// #1670: Mood keeps the lamp's one breath. It begins where the plain
/// breath is, goes on at the pace (the plain pace calm by day, quicker
/// after a bang), never jumps when the state changes, and a reader moving
/// it on from the last state said lands where Mood itself does.
#[test]
fn the_breath_goes_on_at_the_pace_and_never_jumps() {
    use lamp_msgs::{MOOD_BREATH_US, MOOD_PACE_SLOWEST, plain_breath_phase};
    let phase = |engine: &Engine| f64::from(state(engine).breath.unwrap().phase);
    let pace = |engine: &Engine| state(engine).breath.unwrap().pace;
    let mut mood = Engine::new("host-boot".into(), 7 * SECOND_US, DAY).unwrap();
    assert!((phase(&mood) - plain_breath_phase(7 * SECOND_US)).abs() < 1e-6);
    assert_eq!(pace(&mood), 1.0);
    // Calm by day: a second is a 5.2th of a breath.
    let before = state(&mood);
    mood.advance(8 * SECOND_US, DAY).unwrap();
    let step = SECOND_US as f64 / MOOD_BREATH_US as f64;
    let moved = (phase(&mood) - f64::from(before.breath.unwrap().phase)).rem_euclid(1.0);
    assert!((moved - step).abs() < 1e-5, "{moved}");
    // A reader moving the earlier state on a second lands there too.
    let (read, _) = before.breath_at(before.stamp.at_us + SECOND_US).unwrap();
    assert!((read - phase(&mood)).abs() < 1e-5, "{read}");
    // A bang: energy up, the pace quicker, and the breath where it was.
    let at_bang = phase(&mood);
    send(&mut mood, 8 * SECOND_US, EventKind::SuddenSound);
    assert!((phase(&mood) - at_bang).abs() < 1e-6);
    let quick = pace(&mood);
    assert!(quick > 1.1, "{quick}");
    mood.advance(9 * SECOND_US, DAY).unwrap();
    let moved = (phase(&mood) - at_bang).rem_euclid(1.0);
    assert!((moved - step * f64::from(quick)).abs() < 1e-5, "{moved}");
    // Asleep at night: the slowest.
    let night = Engine::new("host-boot".into(), 0, NIGHT).unwrap();
    assert_eq!(pace(&night), MOOD_PACE_SLOWEST);
}

/// #1671: two `positive` reads in a row from the near face are a smile. The
/// lamp brightens a little, half a bang's energy and laughter's warmth, at
/// most once in its cooldown; a smile held on doesn't add again.
#[test]
fn a_smile_brightens_the_lamp_a_little_once_in_its_cooldown() {
    use lamp_mood::engine::SMILE_COOLDOWN_US;
    use lamp_msgs::SeemsKind::{Positive, Unsure};
    let mut mood = engine(DAY);
    send(
        &mut mood,
        SECOND_US,
        EventKind::Faces(Some(face(BOOT, "a"))),
    );
    send(&mut mood, 2 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.30);
    send(&mut mood, 4 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.40);
    close(state(&mood).warmth, 0.63);
    // Held on: no more, only the pulse fading over its 15 s half-life.
    send(&mut mood, 6 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.30 + 0.10 * 0.5f32.powf(2.0 / 15.0));
    // An unsure read breaks the run, and a smile inside the cooldown adds
    // nothing.
    send(&mut mood, 8 * SECOND_US, EventKind::Seems(None));
    send(&mut mood, 10 * SECOND_US, EventKind::Seems(Some(Positive)));
    send(&mut mood, 12 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.30 + 0.10 * 0.5f32.powf(8.0 / 15.0));
    // After it, a smile counts again.
    let later = 4 * SECOND_US + SMILE_COOLDOWN_US;
    send(&mut mood, later, EventKind::Seems(Some(Unsure)));
    send(
        &mut mood,
        later + SECOND_US,
        EventKind::Seems(Some(Positive)),
    );
    let base = state(&mood).energy;
    send(
        &mut mood,
        later + 2 * SECOND_US,
        EventKind::Seems(Some(Positive)),
    );
    let added = state(&mood).energy - base;
    assert!((added - 0.10).abs() < 0.001, "{added}");
}

/// One read is no smile, and reads of two kinds don't add up to one.
#[test]
fn one_read_or_a_broken_run_is_no_smile() {
    use lamp_msgs::SeemsKind::{Negative, Positive};
    let mut mood = engine(DAY);
    send(
        &mut mood,
        SECOND_US,
        EventKind::Faces(Some(face(BOOT, "a"))),
    );
    send(&mut mood, 2 * SECOND_US, EventKind::Seems(Some(Positive)));
    send(&mut mood, 4 * SECOND_US, EventKind::Seems(Some(Negative)));
    send(&mut mood, 6 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.30);
    close(state(&mood).warmth, 0.60);
}

/// #1671: three `negative` reads in a row and the lamp keeps someone low
/// company quietly: its energy a step lower and a startle's let go, for two
/// minutes, while a face is near. A smile, or nobody near, ends it early.
#[test]
fn someone_low_quiets_the_lamp_for_two_minutes_unless_they_smile_or_go() {
    use lamp_mood::engine::QUIET_US;
    use lamp_msgs::SeemsKind::{Negative, Positive};
    // A face the eyes keep seeing, renewed every 10 s for 15.
    let near = |mood: &mut Engine, at: u64| {
        mood.apply(
            at,
            leased(
                at,
                MAX_EVENT_LEASE_US,
                EventKind::Faces(Some(face(BOOT, "a"))),
            ),
        )
        .unwrap();
    };
    let low = |mood: &mut Engine, from: u64| {
        for read in 0..3 {
            send(
                mood,
                from + read * 2 * SECOND_US,
                EventKind::Seems(Some(Negative)),
            );
        }
    };
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::SuddenSound);
    assert!(state(&mood).energy > 0.50);
    low(&mut mood, 3 * SECOND_US);
    close(state(&mood).energy, 0.20);
    let set = 7 * SECOND_US;
    let mut at = 10 * SECOND_US;
    while at < set + QUIET_US {
        near(&mut mood, at);
        at += 10 * SECOND_US;
    }
    mood.advance(set + QUIET_US - 1, DAY).unwrap();
    close(state(&mood).energy, 0.20);
    mood.advance(set + QUIET_US, DAY).unwrap();
    close(state(&mood).energy, 0.30);

    // A smile ends it.
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    low(&mut mood, 2 * SECOND_US);
    close(state(&mood).energy, 0.20);
    send(&mut mood, 8 * SECOND_US, EventKind::Seems(Some(Positive)));
    send(&mut mood, 10 * SECOND_US, EventKind::Seems(Some(Positive)));
    close(state(&mood).energy, 0.40);

    // Nobody near ends it, said or by the face's lease ending, and their
    // face coming back doesn't bring it back.
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    low(&mut mood, 2 * SECOND_US);
    send(&mut mood, 8 * SECOND_US, EventKind::Faces(None));
    close(state(&mood).energy, 0.30);
    near(&mut mood, 9 * SECOND_US);
    close(state(&mood).energy, 0.30);
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    low(&mut mood, 2 * SECOND_US);
    mood.advance(SECOND_US + MAX_EVENT_LEASE_US, DAY).unwrap();
    near(&mut mood, 20 * SECOND_US);
    close(state(&mood).energy, 0.30);
}

/// Asleep, how a face seems changes nothing.
#[test]
fn a_sleeping_lamp_takes_nothing_from_how_a_face_seems() {
    use lamp_msgs::SeemsKind::{Negative, Positive};
    let mut mood = engine(NIGHT);
    assert!(state(&mood).asleep);
    // Checked after the smiles and again after the low reads: the low
    // reads end any pulse, so a smile taken would be gone by then (the
    // reviewer's P3 1 on #2254).
    for kinds in [
        [(1, Positive), (3, Positive)].as_slice(),
        &[(5, Negative), (7, Negative), (9, Negative)],
    ] {
        for &(at, kind) in kinds {
            send(&mut mood, at * SECOND_US, EventKind::Seems(Some(kind)));
        }
        let value = state(&mood);
        close(value.energy, 0.30);
        close(value.warmth, 0.60);
        assert!(value.asleep);
    }
}

fn nudge(energy_milli: i16, warmth_milli: u16, until_us: u64) -> EventKind {
    EventKind::Nudge {
        energy_milli,
        warmth_milli,
        until_us,
    }
}

/// An add-on's kept nudge (#2530): all of it until its last minute, then
/// fading to nothing at its end.
#[test]
fn an_add_ons_nudge_holds_then_fades_over_its_last_minute() {
    let mut mood = engine(DAY);
    let until = 10 * MINUTE_US;
    send(&mut mood, 1, nudge(100, 100, until));
    close(state(&mood).energy, 0.40);
    close(state(&mood).warmth, 0.70);
    mood.advance(until - NUDGE_FADE_US, DAY).unwrap();
    close(state(&mood).energy, 0.40);
    mood.advance(until - NUDGE_FADE_US / 2, DAY).unwrap();
    close(state(&mood).energy, 0.35);
    close(state(&mood).warmth, 0.65);
    mood.advance(until, DAY).unwrap();
    close(state(&mood).energy, 0.30);
    close(state(&mood).warmth, 0.60);
}

/// The first builder's bound: no nudge moves energy or warmth past the
/// clamps that hold without it, and none moves either by more than 0.1.
#[test]
fn a_nudge_never_moves_energy_or_warmth_past_its_clamp() {
    let until = 10 * MINUTE_US;
    // Down: energy never below 0, and warmth never below its neutral.
    let mut low = engine(DAY);
    send(&mut low, 1, nudge(-30_000, 0, until));
    close(state(&low).energy, 0.20);
    close(state(&low).warmth, 0.60);
    // Up, from as high as the lamp gets by itself.
    let mut high = engine(DAY);
    send(&mut high, 1, EventKind::SuddenSound);
    send(&mut high, 1 + SUDDEN_COOLDOWN_US, EventKind::SuddenSound);
    let begun = 2 + SUDDEN_COOLDOWN_US;
    for step in 0..40 {
        send(
            &mut high,
            begun + step * WARMTH_COOLDOWN_US,
            EventKind::Touch { kind: true },
        );
    }
    let at = begun + 40 * WARMTH_COOLDOWN_US;
    send(&mut high, at, EventKind::SuddenSound);
    let plain = state(&high);
    send(&mut high, at + 1, nudge(30_000, 30_000, at + until));
    let value = state(&high);
    assert!(value.energy <= 0.70, "{}", value.energy);
    assert!(value.energy - plain.energy <= 0.100_001, "{value:?}");
    close(value.warmth, 0.85);
}

#[test]
fn the_newest_nudge_replaces_the_last_and_sleep_takes_none() {
    let mut mood = engine(DAY);
    send(&mut mood, 1, nudge(100, 0, 10 * MINUTE_US));
    send(&mut mood, 2, nudge(-50, 20, 10 * MINUTE_US));
    close(state(&mood).energy, 0.25);
    close(state(&mood).warmth, 0.62);
    let mut asleep = engine(NIGHT);
    send(&mut asleep, 1, nudge(100, 100, 10 * MINUTE_US));
    close(state(&asleep).energy, 0.30);
    close(state(&asleep).warmth, 0.60);
}

/// #1722, the voice half of "Reads your mood": a line's tone counts against
/// its bars. Down words are low whatever the voice; glad words are glad
/// unless the voice was surely flat; with no such words, only a surely
/// lively voice counts. Calm alone is nothing, and a value that isn't a
/// number counts for nothing.
#[test]
fn a_lines_tone_counts_only_past_its_bars() {
    use lamp_mood::engine::{ToneKind::*, tone_kind};
    assert_eq!(tone_kind(0.0, 0.9, Some(0.8)), Some(Glad));
    assert_eq!(tone_kind(-0.5, 0.9, Some(0.8)), None);
    assert_eq!(tone_kind(-0.5, 0.3, Some(0.8)), Some(Glad));
    assert_eq!(tone_kind(0.9, 0.9, Some(-0.6)), Some(Low));
    assert_eq!(tone_kind(-0.9, 0.0, Some(-1.0)), Some(Low));
    assert_eq!(tone_kind(0.7, 0.8, None), Some(Lively));
    assert_eq!(tone_kind(0.7, 0.4, None), None);
    assert_eq!(tone_kind(0.7, 0.8, Some(0.2)), None);
    assert_eq!(tone_kind(-0.8, 0.9, None), None);
    assert_eq!(tone_kind(0.0, 0.9, Some(0.4)), None);
    assert_eq!(tone_kind(0.0, 0.9, Some(-0.4)), None);
    assert_eq!(tone_kind(f32::NAN, 0.9, None), None);
    assert_eq!(tone_kind(0.9, f32::NAN, None), None);
    assert_eq!(tone_kind(0.0, 0.9, Some(f32::NAN)), None);
}

/// A glad line brightens the lamp as a smile does, and shares its cooldown:
/// a smile just after adds nothing. A lively line adds half a smile's energy
/// and no warmth, inside the same cooldown.
#[test]
fn a_glad_or_lively_line_brightens_the_lamp_once_in_the_smiles_cooldown() {
    use lamp_mood::engine::{SMILE_COOLDOWN_US, ToneKind};
    use lamp_msgs::SeemsKind::Positive;
    let mut mood = engine(DAY);
    send(
        &mut mood,
        SECOND_US,
        EventKind::Faces(Some(face(BOOT, "a"))),
    );
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Glad));
    close(state(&mood).energy, 0.40);
    close(state(&mood).warmth, 0.63);
    // A smile right after, and a lively line: nothing more.
    send(&mut mood, 3 * SECOND_US, EventKind::Seems(Some(Positive)));
    send(&mut mood, 4 * SECOND_US, EventKind::Seems(Some(Positive)));
    send(&mut mood, 5 * SECOND_US, EventKind::Tone(ToneKind::Lively));
    close(state(&mood).energy, 0.30 + 0.10 * 0.5f32.powf(3.0 / 15.0));
    // Warmth only fading from the glad line's, over its 20 min half-life.
    let warmth = state(&mood).warmth;
    assert!(warmth <= 0.63 && warmth > 0.629, "{warmth}");
    // After the cooldown, a lively line: half the energy, no warmth.
    let later = 2 * SECOND_US + SMILE_COOLDOWN_US;
    send(&mut mood, later, EventKind::Faces(Some(face(BOOT, "a"))));
    let base = state(&mood).energy;
    let warmth = state(&mood).warmth;
    send(
        &mut mood,
        later + SECOND_US,
        EventKind::Tone(ToneKind::Lively),
    );
    let added = state(&mood).energy - base;
    assert!((added - 0.05).abs() < 0.002, "{added}");
    assert!(state(&mood).warmth <= warmth, "no warmth added");
    // And a smile inside its cooldown adds nothing.
    send(
        &mut mood,
        later + 2 * SECOND_US,
        EventKind::Seems(Some(Positive)),
    );
    send(
        &mut mood,
        later + 3 * SECOND_US,
        EventKind::Seems(Some(Positive)),
    );
    let after = state(&mood).energy - base;
    assert!(after < 0.05, "{after}");
}

/// A down line keeps someone near company quietly for two minutes, as a low
/// face does; a glad line ends it. With no face near, it changes nothing.
#[test]
fn a_down_line_quiets_the_lamp_while_a_face_is_near_until_a_glad_one() {
    use lamp_mood::engine::{QUIET_US, ToneKind};
    let near = |mood: &mut Engine, at: u64| {
        mood.apply(
            at,
            leased(
                at,
                MAX_EVENT_LEASE_US,
                EventKind::Faces(Some(face(BOOT, "a"))),
            ),
        )
        .unwrap();
    };
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::SuddenSound);
    assert!(state(&mood).energy > 0.50);
    send(&mut mood, 3 * SECOND_US, EventKind::Tone(ToneKind::Low));
    close(state(&mood).energy, 0.20);
    let mut at = 10 * SECOND_US;
    while at < 3 * SECOND_US + QUIET_US {
        near(&mut mood, at);
        at += 10 * SECOND_US;
    }
    mood.advance(3 * SECOND_US + QUIET_US - 1, DAY).unwrap();
    close(state(&mood).energy, 0.20);
    mood.advance(3 * SECOND_US + QUIET_US, DAY).unwrap();
    close(state(&mood).energy, 0.30);

    // A glad line ends it.
    let mut mood = engine(DAY);
    near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Low));
    close(state(&mood).energy, 0.20);
    send(&mut mood, 4 * SECOND_US, EventKind::Tone(ToneKind::Glad));
    close(state(&mood).energy, 0.40);

    // No face near: nothing, and nothing kept for a face that sits down
    // after (the reviewer-lead's P3 on #2570).
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Tone(ToneKind::Low));
    close(state(&mood).energy, 0.30);
    near(&mut mood, 31 * SECOND_US);
    close(state(&mood).energy, 0.30);
    mood.advance(60 * SECOND_US, DAY).unwrap();
    close(state(&mood).energy, 0.30);
}

/// Asleep, how a line sounded changes nothing.
#[test]
fn a_sleeping_lamp_takes_nothing_from_how_a_line_sounded() {
    use lamp_mood::engine::ToneKind::*;
    let mut mood = engine(NIGHT);
    assert!(state(&mood).asleep);
    for (at, kind) in [(1, Glad), (3, Lively), (5, Low)] {
        send(&mut mood, at * SECOND_US, EventKind::Tone(kind));
        let value = state(&mood);
        close(value.energy, 0.30);
        close(value.warmth, 0.60);
        assert!(value.asleep);
    }
}

/// Each sense makes its own (Ten's 10:47 on #1722): a line's tone neither
/// counts toward the face's run of reads nor breaks it. Two low face reads
/// and a line that isn't low are no quiet, and the face's third low read
/// after that line still is one.
#[test]
fn a_lines_tone_neither_adds_to_nor_breaks_the_faces_run() {
    use lamp_mood::engine::ToneKind;
    use lamp_msgs::SeemsKind::Negative;
    let mut mood = engine(DAY);
    mood.apply(
        SECOND_US,
        leased(
            SECOND_US,
            MAX_EVENT_LEASE_US,
            EventKind::Faces(Some(face(BOOT, "a"))),
        ),
    )
    .unwrap();
    send(&mut mood, 2 * SECOND_US, EventKind::Seems(Some(Negative)));
    send(&mut mood, 4 * SECOND_US, EventKind::Seems(Some(Negative)));
    // A lively line between: half a smile's energy, and no quiet.
    send(&mut mood, 5 * SECOND_US, EventKind::Tone(ToneKind::Lively));
    close(state(&mood).energy, 0.35);
    // The face's third low read still makes its quiet.
    send(&mut mood, 6 * SECOND_US, EventKind::Seems(Some(Negative)));
    close(state(&mood).energy, 0.20);
}

/// Ten's clamp (#1722, 16:1x): glad and lively lines, with bangs and a laugh
/// between them, over an hour of smile cooldowns with someone near, never
/// take energy past its cap or warmth past its own. Warmth presses its cap;
/// energy peaks a bang's 0.25 and a line's 0.10 over its 0.30, below it.
#[test]
fn glad_and_lively_lines_with_bangs_never_pass_the_caps() {
    use lamp_mood::engine::{SMILE_COOLDOWN_US, ToneKind};
    let mut mood = engine(DAY);
    let (mut energy, mut warmth) = (0.0f32, 0.0f32);
    let mut at = SECOND_US;
    for round in 0..30u64 {
        let line = if round % 2 == 0 {
            ToneKind::Glad
        } else {
            ToneKind::Lively
        };
        mood.apply(
            at,
            leased(
                at,
                MAX_EVENT_LEASE_US,
                EventKind::Faces(Some(face(BOOT, "a"))),
            ),
        )
        .unwrap();
        for (offset, kind) in [
            (SECOND_US, EventKind::Tone(line)),
            (2 * SECOND_US, EventKind::SuddenSound),
            (3 * SECOND_US, EventKind::Tone(ToneKind::Glad)),
            (15 * SECOND_US, EventKind::Laughter),
            (16 * SECOND_US, EventKind::SuddenSound),
        ] {
            send(&mut mood, at + offset, kind);
            let value = state(&mood);
            assert!(value.energy <= 0.70, "round {round}: {}", value.energy);
            assert!(value.warmth <= 0.85, "round {round}: {}", value.warmth);
            energy = energy.max(value.energy);
            warmth = warmth.max(value.warmth);
        }
        at += SMILE_COOLDOWN_US + SECOND_US;
    }
    close(warmth, 0.85);
    assert!(energy > 0.64, "{energy}");
}

/// Tone changes the transient mood, while an add-on keeps its own deadline.
/// Their combined energy still obeys the cap, and a down line clears the
/// transient without clearing or extending the nudge.
#[test]
fn a_lines_tone_keeps_a_nudges_deadline_and_the_shared_caps() {
    use lamp_mood::engine::ToneKind;
    let mut mood = engine(DAY);
    let at = SECOND_US;
    let until = at + 2 * MINUTE_US;
    let near = |mood: &mut Engine, at| {
        mood.apply(
            at,
            leased(
                at,
                MAX_EVENT_LEASE_US,
                EventKind::Faces(Some(face(BOOT, "a"))),
            ),
        )
        .unwrap();
    };
    near(&mut mood, at);
    send(&mut mood, at, nudge(100, 100, until));
    send(&mut mood, at, EventKind::Tone(ToneKind::Glad));
    send(&mut mood, at, EventKind::SuddenSound);
    close(state(&mood).energy, 0.70);
    close(state(&mood).warmth, 0.73);

    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Low));
    close(state(&mood).energy, 0.30);
    for at in (10..=120).step_by(10) {
        near(&mut mood, at * SECOND_US);
        if at == 90 {
            mood.advance(until - NUDGE_FADE_US / 2, DAY).unwrap();
            close(state(&mood).energy, 0.25);
        }
    }
    mood.advance(until, DAY).unwrap();
    close(state(&mood).energy, 0.20);
    mood.advance(until + SECOND_US, DAY).unwrap();
    close(state(&mood).energy, 0.30);
}

/// A face the eyes keep seeing, its lease the longest an event may have.
fn felt_near(mood: &mut Engine, at: u64) {
    mood.apply(
        at,
        leased(
            at,
            MAX_EVENT_LEASE_US,
            EventKind::Faces(Some(face(BOOT, "a"))),
        ),
    )
    .unwrap();
}

fn felt(kind: lamp_msgs::FeltKind, until_us: u64) -> Option<lamp_msgs::Felt> {
    Some(lamp_msgs::Felt { kind, until_us })
}

/// #3691: each evidence says what the lamp felt. A smile, a glad line or a
/// kindly touch is glad, a surely lively line lively, each held
/// [`FELT_HOLD_US`] from when it was seen; someone low with a face near is
/// low, for the two-minute low spell.
#[test]
fn each_evidence_says_what_the_lamp_felt() {
    use lamp_mood::engine::{FELT_HOLD_US, QUIET_US, ToneKind};
    use lamp_msgs::FeltKind::{Glad, Lively, Low};
    use lamp_msgs::SeemsKind::{Negative, Positive};
    assert_eq!(
        FELT_HOLD_US,
        90 * SECOND_US,
        "past the feelings skill's minute after a talk"
    );

    // A smile: on its second positive read, not its first.
    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Seems(Some(Positive)));
    assert_eq!(state(&mood).felt, None);
    send(&mut mood, 4 * SECOND_US, EventKind::Seems(Some(Positive)));
    assert_eq!(state(&mood).felt, felt(Glad, 4 * SECOND_US + FELT_HOLD_US));

    // A glad line, and a lively one.
    for (kind, feeling) in [(ToneKind::Glad, Glad), (ToneKind::Lively, Lively)] {
        let mut mood = engine(DAY);
        send(&mut mood, SECOND_US, EventKind::Tone(kind));
        assert_eq!(state(&mood).felt, felt(feeling, SECOND_US + FELT_HOLD_US));
    }

    // A kindly touch is glad; an attending tap is nothing.
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Touch { kind: false });
    assert_eq!(state(&mood).felt, None);
    send(&mut mood, 2 * SECOND_US, EventKind::Touch { kind: true });
    assert_eq!(state(&mood).felt, felt(Glad, 2 * SECOND_US + FELT_HOLD_US));

    // Low by the face's three reads, or by a down line, with a face near.
    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    for at in [2, 4] {
        send(&mut mood, at * SECOND_US, EventKind::Seems(Some(Negative)));
        assert_eq!(state(&mood).felt, None);
    }
    send(&mut mood, 6 * SECOND_US, EventKind::Seems(Some(Negative)));
    assert_eq!(state(&mood).felt, felt(Low, 6 * SECOND_US + QUIET_US));
    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Low));
    assert_eq!(state(&mood).felt, felt(Low, 2 * SECOND_US + QUIET_US));

    // A down line with nobody near is nothing.
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Tone(ToneKind::Low));
    assert_eq!(state(&mood).felt, None);
}

/// A bang or stuffy air moves energy but is no feeling (#3691).
#[test]
fn a_bang_or_stuffy_air_is_no_feeling() {
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::SuddenSound);
    assert!(state(&mood).energy > 0.50);
    assert_eq!(state(&mood).felt, None);

    let mut mood = engine(DAY);
    let mut at = 0;
    while at <= AIR_QUALIFY_US + 2 * MINUTE_US {
        mood.apply(
            at,
            leased(at, MAX_EVENT_LEASE_US, EventKind::Air(Some(2_000))),
        )
        .unwrap();
        assert_eq!(state(&mood).felt, None, "at {at}");
        at += 10 * SECOND_US;
    }
    assert!(state(&mood).energy < 0.29, "the air lowered energy");
}

/// The hold runs out: glad and lively at [`FELT_HOLD_US`], low with its
/// spell, or sooner when nobody is near or the lamp sleeps.
#[test]
fn a_feeling_runs_out_with_its_hold() {
    use lamp_mood::engine::{FELT_HOLD_US, QUIET_US, ToneKind};
    use lamp_msgs::FeltKind::{Glad, Low};

    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Tone(ToneKind::Glad));
    let until = SECOND_US + FELT_HOLD_US;
    assert!(mood.advance(until - 1, DAY).is_ok());
    assert_eq!(state(&mood).felt, felt(Glad, until));
    // Its end is a change, so Mood says it at once.
    assert!(mood.advance(until, DAY).unwrap());
    assert_eq!(state(&mood).felt, None);

    // Low lasts its two minutes while the face stays...
    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Low));
    let until = 2 * SECOND_US + QUIET_US;
    let mut at = 10 * SECOND_US;
    while at < until {
        felt_near(&mut mood, at);
        at += 10 * SECOND_US;
    }
    mood.advance(until - 1, DAY).unwrap();
    assert_eq!(state(&mood).felt, felt(Low, until));
    mood.advance(until, DAY).unwrap();
    assert_eq!(state(&mood).felt, None);

    // ...and ends when nobody is near, and doesn't come back with the face.
    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Low));
    send(&mut mood, 3 * SECOND_US, EventKind::Faces(None));
    assert_eq!(state(&mood).felt, None);
    felt_near(&mut mood, 4 * SECOND_US);
    assert_eq!(state(&mood).felt, None);

    // Sleep ends any feeling.
    let mut mood = engine(DAY);
    send(&mut mood, SECOND_US, EventKind::Tone(ToneKind::Glad));
    send(&mut mood, 2 * SECOND_US, EventKind::Sleep);
    assert!(state(&mood).asleep);
    assert_eq!(state(&mood).felt, None);
}

/// A newer feeling replaces an older one, even inside the smile's cooldown,
/// when the energy doesn't move again; low gives way only to a feeling that
/// ends its spell.
#[test]
fn a_newer_feeling_replaces_an_older_one() {
    use lamp_mood::engine::{FELT_HOLD_US, QUIET_US, ToneKind};
    use lamp_msgs::FeltKind::{Glad, Lively, Low};

    let mut mood = engine(DAY);
    felt_near(&mut mood, SECOND_US);
    send(&mut mood, 2 * SECOND_US, EventKind::Tone(ToneKind::Glad));
    send(&mut mood, 3 * SECOND_US, EventKind::Tone(ToneKind::Lively));
    assert_eq!(
        state(&mood).felt,
        felt(Lively, 3 * SECOND_US + FELT_HOLD_US)
    );
    send(&mut mood, 4 * SECOND_US, EventKind::Tone(ToneKind::Low));
    assert_eq!(state(&mood).felt, felt(Low, 4 * SECOND_US + QUIET_US));
    // Only a feeling that ends the low spell replaces low.
    send(&mut mood, 5 * SECOND_US, EventKind::Touch { kind: true });
    assert_eq!(state(&mood).felt, felt(Low, 4 * SECOND_US + QUIET_US));
    send(&mut mood, 6 * SECOND_US, EventKind::Tone(ToneKind::Glad));
    assert_eq!(state(&mood).felt, felt(Glad, 6 * SECOND_US + FELT_HOLD_US));
}

/// A lively line in a low spell doesn't replace low: keeping someone low
/// company wins, and that voice may not be theirs (the reviewer-lead's P3
/// on #3725). Three low reads at 0 and a lively line at 30 s stay low to
/// the spell's end.
#[test]
fn a_lively_line_does_not_end_low_company() {
    use lamp_mood::engine::{QUIET_US, ToneKind};
    use lamp_msgs::FeltKind::Low;
    use lamp_msgs::SeemsKind::Negative;

    let mut mood = engine(DAY);
    felt_near(&mut mood, 0);
    // Three reads at 0, a microsecond apart as their stream needs.
    for at in 0..3 {
        send(&mut mood, at, EventKind::Seems(Some(Negative)));
    }
    let until = 2 + QUIET_US;
    assert_eq!(state(&mood).felt, felt(Low, until));
    let mut at = 10 * SECOND_US;
    while at < QUIET_US + 20 * SECOND_US {
        felt_near(&mut mood, at);
        if at == 30 * SECOND_US {
            send(&mut mood, at, EventKind::Tone(ToneKind::Lively));
        }
        mood.advance(at, DAY).unwrap();
        if at < until {
            assert_eq!(state(&mood).felt, felt(Low, until), "at {at}");
        } else {
            assert_eq!(state(&mood).felt, None, "at {at}");
        }
        at += 10 * SECOND_US;
    }
}

/// A kindly touch in a low spell keeps low too: a hand on the lamp while
/// someone is low is comfort, not glad. Only a feeling that ends the spell
/// replaces low.
#[test]
fn a_kindly_touch_keeps_low_company() {
    use lamp_mood::engine::QUIET_US;
    use lamp_msgs::FeltKind::Low;
    use lamp_msgs::SeemsKind::Negative;

    let mut mood = engine(DAY);
    felt_near(&mut mood, 0);
    for at in 0..3 {
        send(&mut mood, at, EventKind::Seems(Some(Negative)));
    }
    let until = 2 + QUIET_US;
    let warmth = state(&mood).warmth;
    for at in [10, 20, 30, 40, 50] {
        let at = at * SECOND_US;
        felt_near(&mut mood, at);
        if at == 30 * SECOND_US {
            send(&mut mood, at, EventKind::Touch { kind: true });
            assert!(state(&mood).warmth > warmth, "its warmth still lifts");
        }
        mood.advance(at, DAY).unwrap();
        assert_eq!(state(&mood).felt, felt(Low, until), "at {at}");
    }
}
