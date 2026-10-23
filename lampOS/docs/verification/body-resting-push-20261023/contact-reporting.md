# Contact reporting during a glance

A second P2 in #3196 at `171c6a996` is reproduced on the Mac. The new
reporting predicate compares load with the gesture-expanded `strained`
bound (700). The independent pitch contact floor is 650: sustained load
675 pins the joint and later makes it slump, yet the proposed predicate
never says a hand while that pinned contact is present.

On `e5cd73aca` plus this fixture, the case passed **1/0**. With only
#3196's production diff applied to `body.rs`, the same case failed **0/1**,
exit 101, at the `g.hand` assertion. The failed sample has pitch load 675,
position and goal 1,940, `eased = [1, 0, 0]`, `released = 0`, `limp = false`
and `hand = false`. Earlier assertions establish contact before the
ordinary six-read load threshold and the eventual pressure-induced slump.

The fixture keeps a 40-count requested glance fresh from 10 to 14 seconds
and applies pitch load 675 from 11 to 13 seconds. Every sample in the
nonempty 11.2 to 11.9 second fixture-clock window, before slump, must retain
the load and say hand. These are simulated fixture times; the runtime's
step counter has its own origin. This is a reporting regression, not a
failure of the existing physical contact stop, and not a new device run.

The new `felt` logic must retain evidence from independent contact
protection as well as the sustained gentle-push control. No product fix is
included in this receipt. Linux, ARM64 and physical behavior were not run
for this review.

Command from the isolated worktree's `lampOS/`, with
`CARGO_TARGET_DIR=/private/tmp/third-body-gates-target` and
`CARGO_BUILD_JOBS=2`:

```sh
cargo test -q --offline --locked -p lamp-body --lib a_pitch_contact_during_glance_cooling_is_said_as_a_hand_before_it_slumps -- --test-threads=2
```

Replace the existing `rest_glances` helper with the following wrapper and
observed helper, then add the following test in `body/tests.rs`. All other
fixture definitions are unchanged. Existing callers use a no-op observer.

```rust
/// The same with these glances: from, until and the pitch asked.
fn rest_glances(
    set: impl FnOnce(&mut Lamp, &mut Limits),
    glances: Vec<(Duration, Duration, f32)>,
) -> (BodyReport, Vec<(Duration, u16, u16)>) {
    rest_glances_watched(set, glances, |_, _| {})
}

/// The same fixture, with each full step available to reporting tests.
fn rest_glances_watched(
    set: impl FnOnce(&mut Lamp, &mut Limits),
    glances: Vec<(Duration, Duration, f32)>,
    mut watch: impl FnMut(Duration, &Glimpse),
) -> (BodyReport, Vec<(Duration, u16, u16)>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(16));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    // Found standing, as the factory's self-test finds it, its head at its
    // goal there.
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_015, 2_650, 2_600, 2_044, 1_900])
    {
        servo.position = f32::from(at);
        servo.goal = at;
    }
    let mut limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .map(|limits| Limits {
        rest_glance: true,
        ..limits
    })
    .unwrap();
    set(&mut lamp, &mut limits);
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Resting, 0)),
        move || Asked {
            toward: None,
            seen: None,
            gesture: None,
            expression: glances
                .iter()
                .find(|(from, until, _)| (*from..*until).contains(&time.get()))
                .map(|(_, _, pitch)| [*pitch, 0.0, 0.0]),
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            asleep: false,
            woken: false,
        },
        |glimpse| {
            let at = seen_at.get();
            seen.push((at, glimpse.goals[0], glimpse.positions[0]));
            watch(at, glimpse);
        },
    );
    (report, seen)
}

/// A stationary hand can cross the pitch's contact floor while a glance
/// permits a higher motion load. Contact still pins it, and the hand must
/// be reported throughout that hold before sustained pressure makes it slump.
#[test]
fn a_pitch_contact_during_glance_cooling_is_said_as_a_hand_before_it_slumps() {
    let ms = Duration::from_millis;
    let pressed = GLANCE_UNTIL;
    let load = 675;
    let mut seen = Vec::new();
    let (report, _) = rest_glances_watched(
        |lamp, limits| {
            assert!(limits.load[0] < load);
            assert!(CONTACT_FLOOR[0] < load && load <= GESTURE_LOAD);
            limits.give_way = Some(GiveWay::Return);
            limits.limp = true;
            lamp.strains
                .push((pressed, pressed + ms(2_000), PITCH, load));
        },
        vec![(GLANCE_FROM, pressed + ms(3_000), 40.0)],
        |at, glimpse| seen.push((at, *glimpse)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.expression_steps > 0, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0], "{report:?}");
    assert_eq!(report.released_by_hand, [1, 0, 0], "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.released[0].joint, Joint::WristPitch.name());
    assert_eq!(report.released[0].load, load);

    // Contact's three reads pin it before the ordinary load guard's six.
    let first = seen.iter().find(|(_, g)| g.eased[0] == 1).unwrap();
    assert!(
        first.0 >= pressed && first.0 < pressed + TICK * u32::from(Limits::FOLDED.load_steps),
        "first contact at {:?}, pressed at {pressed:?}",
        first.0
    );
    let window = pressed + ms(200)..pressed + ms(900);
    assert!(Duration::from_secs_f32(report.released[0].seconds) > window.end);
    let held: Vec<_> = seen.iter().filter(|(at, _)| window.contains(at)).collect();
    assert!(!held.is_empty(), "no steps before the slump");
    for (_, g) in held {
        assert!(!g.limp && g.released == 0 && g.eased == [1, 0, 0], "{g:?}");
        assert_eq!(g.loads[0], load, "{g:?}");
        assert!(g.positions[0].abs_diff(first.1.positions[0]) <= HAND_MOVED);
        assert!(g.goals[0].abs_diff(g.positions[0]) <= HAND_MOVED);
        assert!(g.hand, "the pinned contact was not reported: {g:?}");
    }
}
```
