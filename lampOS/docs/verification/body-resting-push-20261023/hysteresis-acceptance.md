# Reporting a continuing hand and its release

First's [10:52 acceptance decision](https://github.com/autonomous-ai/lampOS/pull/3196#issuecomment-6356949897)
requires a qualified push to remain reported while displacement exceeds its
deadband or load remains above a lower hold level, such as half the bound.
It must clear within one control tick once both have cleared. A resting
joint inside its deadband must not start a push. The two tests below make
the lower-load and release requirements explicit; they are new acceptance
requirements, not claims of regressions from the earlier Body.

## Intermediate-head result

On the Mac, the existing review worktree at `e5cd73aca` plus only the
production `body.rs` diff from `d4c6dbeeb` to #3196's intermediate head
`c11d067e8` gave these results. The added review fixtures were identical
across the compared versions. This is not an exact checkout of that PR,
a whole-suite result, a Linux run or a device result.

| Focused case | Passed / failed | Exit | Observation |
|---|---:|---:|---|
| Eight-second displaced gentle hold | 1 / 0 | 0 | Stable initial give-way reference fixes the original sustained-push failure. |
| Glance-cooling contact floor | 0 / 1 | 101 | Step 535 has load 675, a held pitch and no hand report. |
| Release after stationary pressure | 0 / 1 | 101 | Step 227, one step after the first quiet read at step 226, still reports a hand at load 25 and position equal to goal. |
| Lower load after an admitted push | 0 / 1 | 101 | The late hold window at load 130, after entry at 300, reports no hand; pitch's ordinary bound is 240. |

The first two fixtures are in the [main receipt](README.md) and
[contact receipt](contact-reporting.md). The original production patch
`171c6a996` failed both; the unmodified `e5cd73aca` baseline passed them.

Head `8c76a3dab` was published while these checks ran. It adds contact-floor
qualification and requires its own review; the intermediate failures above
must not be attributed to that newer head without running it.

Each command ran from the worktree's `lampOS/`, with `CARGO_BUILD_JOBS=2`
and `CARGO_TARGET_DIR=/private/tmp/third-body-gates-target`:

```sh
cargo test -q --offline --locked -p lamp-body --lib a_sustained_gentle_push_is_said_until_the_hand_lets_go -- --test-threads=2
cargo test -q --offline --locked -p lamp-body --lib a_pitch_contact_during_glance_cooling_is_said_as_a_hand_before_it_slumps -- --test-threads=2
cargo test -q --offline --locked -p lamp-body --lib stationary_pressure_clears_within_one_tick_after_load_and_displacement_clear -- --test-threads=2
cargo test -q --offline --locked -p lamp-body --lib stationary_pressure_remains_a_hand_when_it_eases_above_half_the_bound -- --test-threads=2
```

The release case admits stationary pitch pressure at 300, below the contact
floor, for three simulated seconds. On release, the existing fixture reads
25 at its unchanged goal. It permits the first quiet observation to precede
the decision, then requires the next and every later observation to clear.
The retention case enters at 300, falls to 130 for four seconds without
moving, and checks every sample in a nonempty late window. No new push
entry at 130 is requested. Both use the existing fixture and `--limp on`.

## Revised head `8c76a3dab`

The same four cases were run again, with only the newer production diff
applied to the same `e5cd73aca` review baseline. The eight-second displaced
hold and contact-floor cases now both pass **1 / 0**. The release and
lower-load retention cases each still fail **0 / 1**, exit 101, at the
same boundaries. For release, step 226 is the first quiet observation;
steps 227 and 228 also still report a hand at load 25 and zero goal gap.
For retention, the checked load-130 window begins at step 250 and reports
no hand after the established load-300 pressure.

The source still refreshes reporting only above the full bound or contact
floor, or beyond 15 counts from the initial give-way point. Otherwise it
decrements a 50-step history. This fixes neither the required lower hold
level after qualification nor the one-tick release when both inputs clear.
A pressure that was never admitted is a separate case; both tests here
first establish a reported push at load 300.

A [third current-head case](limp-movement-reporting.md), for a 13-count
physical displacement after contact, passes **1 / 0** on the unmodified
baseline and fails **0 / 1** with the proposed production diff. It checks
the limp guard's smaller movement threshold before the slump. No review
clearance is given while these three current-head failures remain.

## Exact PR-head confirmation

To separate these findings from the newer integration baseline, all three
remaining cases were rerun in a detached checkout of exact PR head
`8c76a3dab56b389bc898ee0d7573908fed3dc89b`. Only three tests and diagnostic
output were appended to `body/tests.rs` (200 added lines); every existing
test and production file remained unchanged. The checkout stayed at the
same head throughout, and `git diff --check` passed.

Each case failed **0 / 1**, exit 101, with 421 tests filtered out. Release
still reports true at steps 227 and 228 after the first quiet step 226.
The gentle-hold window has 95 observations, from step 250 through 344;
all 95 report false at pitch load 130, with position equal to goal 2,000.
The 13-count case has 15 false rows while the verified physical hold
continues; its exact times are in the [movement receipt](limp-movement-reporting.md).

Commands and environment are the same filters listed above, plus the
movement case's command. Wall times were 103.943 seconds for release
including compilation, 0.372 seconds for retention and 0.348 seconds for
the movement case. These remain Mac deterministic test results, not
Linux, ARM64 or on-device evidence.

The command-line defaults also matter when judging applicability. Both
`8c76a3dab` and deployed source `f012c4725` set absent `--give-way` to
`Some(GiveWay::Return)` and absent `--limp` to on
(`bin/lamp-body.rs:568-569`; the same code is at 580-581 in `51186d5ea`).
An argv without either flag therefore does not establish that they are
off. This is a source check; no fresh lamp inspection was performed.

## Complete acceptance cases

```rust
/// The release acceptance rule: once both load and displacement are gone,
/// the hand report clears within one control tick. This is a new reporting
/// requirement, not a claim about how long the earlier body kept a hand.
#[test]
fn stationary_pressure_clears_within_one_tick_after_load_and_displacement_clear() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(7));
    // Above pitch's ordinary bound, below its contact floor: no motion,
    // no physical hand displacement and no contact-floor slump. Releasing
    // the pressure restores the fixture's quiet load of 25 at the goal.
    lamp.strains = vec![(ms(1_500), ms(4_500), PITCH, 300)];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let pressed: Vec<_> = seen
        .iter()
        .filter(|g| (ms(2_500)..ms(4_400)).contains(&clock_of(g)))
        .collect();
    assert!(!pressed.is_empty());
    assert!(
        pressed
            .iter()
            .all(|g| { g.loads[0] == 300 && g.positions[0] == g.goals[0] && g.hand }),
        "stationary pressure was not reported: {pressed:?}"
    );
    let first_pressed = seen.iter().position(|g| g.loads[0] == 300).unwrap();
    let quiet_at = seen
        .iter()
        .enumerate()
        .skip(first_pressed + 1)
        .find(|(_, g)| g.loads[0] == 25 && g.positions[0] == g.goals[0])
        .map(|(index, _)| index)
        .expect("the pressure never cleared at the pinned goal");
    let quiet = &seen[quiet_at..];
    assert!(
        quiet.len() >= 2,
        "no observation after the first quiet read"
    );
    assert_eq!(quiet[1].step, quiet[0].step + 1);
    // `Glimpse` may precede this step's hand decision. Allow that first
    // quiet row, then require every subsequent row to report no hand.
    assert!(
        quiet[1..]
            .iter()
            .all(|g| { g.loads[0] == 25 && g.positions[0] == g.goals[0] && !g.hand }),
        "hand outlasted the first quiet read by more than one tick: {:?}",
        quiet.iter().take(3).collect::<Vec<_>>()
    );
}

/// The hysteresis acceptance rule: a push admitted above the ordinary
/// bound remains a hand above half that bound, even without displacement.
/// This specifies the new retention rule, not the earlier body's policy.
#[test]
fn stationary_pressure_remains_a_hand_when_it_eases_above_half_the_bound() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    // Pitch's bound is 240. First qualify at 300, then hold at 130: below
    // entry but above retention at 120. Its position never leaves its goal.
    lamp.strains = vec![
        (ms(1_500), ms(3_000), PITCH, 300),
        (ms(3_000), ms(7_000), PITCH, 130),
    ];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    for (from, until, load) in [(ms(2_000), ms(2_900), 300), (ms(5_000), ms(6_900), 130)] {
        let held: Vec<_> = seen
            .iter()
            .filter(|g| (from..until).contains(&clock_of(g)))
            .collect();
        assert!(!held.is_empty(), "no observations at load {load}");
        assert!(
            held.iter()
                .all(|g| g.loads[0] == load && g.positions[0] == g.goals[0] && g.hand),
            "stationary pressure at load {load} was not retained: {held:?}"
        );
    }
}
```
