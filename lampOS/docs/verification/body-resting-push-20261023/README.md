# A resting pitch and a moving goal

This investigation follows the 10:00 handoff on Queue #3126. It concerns
false `body/hand` input spoiling head contacts and restarting the rest
clock. The original description, a joint stopped 15 counts short of a
fixed goal, was corrected by the retained trace. No product correction or
lamp qualification is established by this receipt yet.

At 10:29 the implementation was assigned to #3196 and this work became a
review. The proposed production patch is `171c6a996`; the following
regression evidence is against that patch, not an alternative product fix.

A [second contact-reporting regression](contact-reporting.md) also passes
on the baseline and fails with that production patch. It covers pressure
above the independent contact floor while a glance permits a higher motion
load, before the existing protection makes the lamp slump.

The [hysteresis and release acceptance receipt](hysteresis-acceptance.md)
keeps the intermediate `c11d067e8` result and the complete two additional
cases requested by the acceptance decision. Later heads need a fresh read.

## Retained observations

[The excerpt](lamp-06c1-20261022-excerpt.log) preserves all 23 numeric lines
posted in [Queue comment 6356306956](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6356306956).
They cover 21:10 through 21:15:59 on 22 October on lamp-06c1, as kept by a
journal follower. The service started at 21:05:26, before that follower;
the startup interval and its first resistance event were not retained.
This is received historical evidence, not a new device run.

The deployed Body was reported as build 5898, source `f012c4725`, matched
through the artifact's checksum manifest. The reported arguments were:

```text
--force-limit 600 --speed 120 --servo-speed 1000 --servo-acceleration 60
--idle on --rest-glance on --look-at-name on --torque on
--home 1772,2057,2034 --folded-arm 2050,2059
--stand 1772,2057,2034,2657,2602
```

That source defaults to `--give-way return` and `--limp on`. Its logged
three-element goals, positions and loads follow
`COMMANDED = [WristPitch, WristRoll, BaseYaw]` (`body.rs:272`,
`bin/lamp-body.rs:1475`). Element zero is **head pitch**, not base yaw.
The separate five-element bad-read array has another order.

- Pitch position remains 1,765 in the excerpt while its resting goal
  ranges from 1,740 to 1,769. Load follows the goal gap: a 25-count gap
  reads 112; a one-count gap reads zero.
- Step 27,007 changes to Listening, goal 1,757, position 1,765, load 40.
  Step 27,025 reports resistance with load 192 and cumulative count 23.
  The entering goal is absent from that line.
- By step 27,040 the goal is pinned to 1,765 and load is zero. Later
  resting goals move again. There is no continuous 20 ms trace of the
  transition, and no independent indication of whether a person or an
  obstacle was present.

These rows support testing a motor whose load falls when the goal becomes
its position. They do not establish that a specific deadband caused the
first hold. Under the deployed source's default pitch load bound of 240
and tracking bound of 30, the recorded resting gap and load alone do not
enter a hold. A larger unrecorded gap during the Listening transition is
one possibility, not an observed fact.

## Source checks and safety constraints

The existing arrival and rest rules adopt a nearby measured position only
after qualifying stillness. The overload/tracking path can hold a joint
earlier. An existing give-way hold can be prolonged by load above the
lower of its bound and 100, or by movement over 15 counts from its written
goal. `Glimpse.hand` follows that hold. The separate motor-stall detector
does not itself set `hand`.

An error inside 24 counts is insufficient reason to ignore a high load:
a real hand can restrain a joint there. Contact, temperature, overload and
the existing noisy sustained-hand protections must remain effective.
The deterministic fixture must distinguish motor load from constant
external force rather than relabel one as the other.

The maintained V1 at `origin/main` (`1b5f7a78b`) reads actual goal and
present positions for lag. Its contact path distinguishes direct movement
and settling from idle: it temporarily omits off-playback lag floors while
keeping fixed load floors. It pins the current position under the same
lock that blocks subsequent goal writes, then judges continuing load/lag
with bounded post-pin baselines and keeps overload protection
(`hal/drivers/motors/animation_service.py:1240-1299,1417-1467,1578-1648`).
That supports checking motion context while retaining hard protection;
it does not justify a blanket small-error exemption. Its lag unit is a
tenth of a degree, not the Rust runtime's encoder count. No V1 code or
service is introduced into lampOS.

No device access, installation, fresh playback or physical latency
measurement is part of this investigation. The existing Controls
bus-to-recognizer tests already cover fresh push suppression, explicit
release/expiry, a fresh contact after release and primary Stop. The
producer's physical correctness remains a separate requirement.

## Sustained-push regression

A new deterministic case extends the existing gentle-base-push fixture:
the base is moved 40 counts and held at load 120 from 1.5 to 9.5 seconds.
Every sample from 5.0 to 9.4 seconds must retain that position and load
and say `hand = true`. The nonempty 13.0 to 13.9 second window must say
false after release. These are simulated times, not physical latency.

On the Mac, `e5cd73aca` plus the new test passed **1/0**. Applying only
#3196's production diff to `body.rs`, with the same test and base, failed
**0/1**, exit 101. The first checked late sample, step 250 (5 seconds),
has yaw position and goal 2,055, load 120, and `hand = false`. Thus a hand
that originally displaced the joint becomes unreported while still there;
this exceeds the PR's stated trade-off for pressure without displacement.

The cause is the interaction between two existing/new rules: the hold
path periodically adopts the current position as `gave_at`, but the new
`felt` timer renews only from movement against that changing reference or
load above the full bound. The lower sustained-hand load still renews the
motor hold while the reporting timer expires. The review remains held
until a corrected head passes the continuous-hold and release controls.

Two separate `goal_dependent_deadband` controls passed **2/0** on both
versions. They model 24/25-count friction and a load falling with the goal
gap, plus a genuine later push. They deliberately do not invent the
unlogged hold-entry conditions; their passing baseline does not reproduce
the captured device fault.

Commands used the isolated target `/private/tmp/third-body-gates-target`
and `CARGO_BUILD_JOBS=2`, from the isolated worktree's `lampOS/`:

```sh
cargo test -q --offline --locked -p lamp-body --lib a_sustained_gentle_push_is_said_until_the_hand_lets_go -- --test-threads=2
cargo test -q --offline --locked -p lamp-body --lib goal_dependent_deadband -- --test-threads=2
```

The cloned workspace artifacts were invalidated before these runs. An
earlier launch overlapping the cache copy was stopped with exit 143 and
discarded; no result is claimed for it. No device was touched and no
general Body-suite, Linux or ARM64 result is claimed by this review.

The complete added regression, using the existing Body test fixture:

```rust
/// A gentle hand that stays on the base is still said after the joint has
/// repeatedly adopted its position. Reporting must not time out while
/// the same physical push still holds it there under its load bound.
#[test]
fn a_sustained_gentle_push_is_said_until_the_hand_lets_go() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(14));
    let held_at = HOME[2] + 40;
    lamp.hand = Some((ms(1_500), ms(9_500), YAW, f32::from(held_at)));
    lamp.strains = vec![(ms(1_500), ms(9_500), YAW, 120)];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        Some(HOME),
        || Some((Mood::Listening, 100)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[2] >= 1, "the push never gave way: {report:?}");
    // This window begins well after both the first give-way and the
    // subsequent one-second observations of the continuing hand.
    let held: Vec<_> = seen
        .iter()
        .filter(|g| (ms(5_000)..ms(9_400)).contains(&clock_of(g)))
        .collect();
    assert!(!held.is_empty());
    assert!(
        held.iter()
            .all(|g| g.positions[2] == held_at && g.loads[2] == 120),
        "the fixture did not maintain the physical push: {held:?}"
    );
    assert!(
        held.iter().all(|g| g.hand),
        "the continuing hand disappeared: {:?}",
        held.iter().find(|g| !g.hand)
    );
    let released: Vec<_> = seen
        .iter()
        .filter(|g| (ms(13_000)..ms(13_900)).contains(&clock_of(g)))
        .collect();
    assert!(!released.is_empty());
    assert!(released.iter().all(|g| !g.hand), "{released:?}");
}
```
