# A held joint between the reporting and limp deadbands

This checks the remaining movement case raised in the #3196 review:
`LIMP_LAG` is 11 counts, but `HAND_MOVED` is 15. On `8c76a3dab`, a
13-count displacement can therefore keep the limp guard pressed while
`felt` expires and the hand report goes false.

The deterministic fixture first establishes reported pitch contact at
load 675, then removes that load. A simulated physical hand subsequently
holds the joint 13 counts from its pinned goal. The test verifies that the
initial contact was reported, the later goal stays pinned, the position
actually stays 13 counts away, the load is zero, and one pitch-only slump
still occurs after the checked nonempty window. That window starts after
the earlier load evidence has expired. It requires every sample before
the slump to continue reporting the hand.

On the Mac, the `e5cd73aca` review worktree plus this fixture passed
**1 / 0**, exit zero. With only the production `body.rs` diff from
`d4c6dbeeb` to `8c76a3dab` applied, the same fixture failed **0 / 1**,
exit 101. At runtime step 590, pitch goal is 1,900, position 1,913,
load zero, `eased = [1, 0, 0]`, `released = 0` and `hand = false`.
The physical guards still execute; the failure is the reported hand.
The helper's simulated clock has a different origin from `Glimpse.step`.
These are fixture observations, not measurements from a lamp.

## Exact-head timing confirmation

A separate exact checkout of `8c76a3dab` reproduced this failure with
only the three review tests appended. The original test file prefix and
all production files were unchanged. The assertion is unchanged; a compact
pre-assert diagnostic was added to count the whole observation window.
All **15 of 15** observations between 12.306400 and 12.586400 simulated
seconds report no hand, while every row verifies goal 1,900, position
1,913, load zero, no release and no limp. The slump occurs at 12.706400
seconds. Thus the first checked false report is about 400 ms before the
slump, with 280 ms between the first and last checked false samples.

The windows need not start together: this fixture's initial contact load
ends after 200 ms, then a quiet gap precedes the 13-count move at 600 ms.
The quiet gap resets `in_hand`, while `felt` continues aging. Also,
`8c76a3dab` assigns `felt` only in the predicate at `body.rs:5711-5722`;
the give-way step sets `gave_first`, not an unconditional new reporting
timer. The retained exact-head fixture below includes the diagnostic.

Command, from `lampOS/`, with `CARGO_BUILD_JOBS=2` and the isolated
`CARGO_TARGET_DIR=/private/tmp/third-body-gates-target`:

```sh
cargo test -q --offline --locked -p lamp-body --lib a_pinned_pitch_held_thirteen_counts_away_still_says_a_hand_before_slump -- --test-threads=2
```

This uses the complete `rest_glances_watched` helper in the
[contact-reporting receipt](contact-reporting.md), with its existing
`GLANCE_FROM`, `GLANCE_UNTIL`, `PITCH` and servo fixture. No general Body
suite, Linux, ARM64 or device execution is claimed by this focused review.

```rust
/// Once contact has pinned a joint, thirteen counts of displacement keep
/// its limp guard held even without load: above LIMP_LAG, below HAND_MOVED.
/// That continuing physical hold must not disappear from the hand report.
#[test]
fn a_pinned_pitch_held_thirteen_counts_away_still_says_a_hand_before_slump() {
    let ms = Duration::from_millis;
    let pressed = GLANCE_UNTIL;
    let moved = 13;
    let mut pinned_at = 0;
    let mut seen = Vec::new();
    let (report, _) = rest_glances_watched(
        |lamp, limits| {
            assert!(LIMP_LAG < moved && moved <= HAND_MOVED);
            limits.give_way = Some(GiveWay::Return);
            limits.limp = true;
            pinned_at = lamp.servos[PITCH].position.round() as u16;
            lamp.strains = vec![
                (pressed, pressed + ms(200), PITCH, 675),
                (pressed + ms(200), Duration::from_secs(16), PITCH, 0),
            ];
            // After contact stops it, a hand moves the real simulated
            // joint; this is not a changed goal or an injected misreading.
            lamp.hand = Some((
                pressed + ms(600),
                pressed + ms(2_500),
                PITCH,
                f32::from(pinned_at + moved),
            ));
        },
        vec![(GLANCE_FROM, pressed + ms(3_000), 0.0)],
        |at, glimpse| seen.push((at, *glimpse)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0], "{report:?}");
    assert_eq!(report.released_by_hand, [1, 0, 0], "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.released[0].joint, Joint::WristPitch.name());
    assert_eq!(report.released[0].load, 0, "the slump must be from lag");

    let first = seen.iter().find(|(_, g)| g.eased[0] == 1).unwrap();
    assert!(first.0 >= pressed && first.0 < pressed + ms(100));
    assert_eq!(first.1.positions[0], pinned_at);
    // Establish that contact itself was reported before the later move.
    let contact: Vec<_> = seen
        .iter()
        .filter(|(at, _)| (pressed + ms(100)..pressed + ms(180)).contains(at))
        .collect();
    assert!(!contact.is_empty());
    assert!(contact.iter().all(|(_, g)| {
        g.hand && g.eased == [1, 0, 0] && g.loads[0] == 675 && g.positions[0] == pinned_at
    }));

    // The initial load evidence is more than a second old. The real joint
    // remains held thirteen counts from the pinned goal until the slump.
    let window = pressed + ms(1_300)..pressed + ms(1_600);
    assert!(Duration::from_secs_f32(report.released[0].seconds) > window.end);
    let held: Vec<_> = seen.iter().filter(|(at, _)| window.contains(at)).collect();
    assert!(!held.is_empty(), "no held steps before the slump");
    let false_rows: Vec<_> = held
        .iter()
        .filter(|(_, g)| !g.hand)
        .map(|(at, g)| {
            serde_json::json!({
                "at_us": at.as_micros(),
                "step": g.step,
                "goal": g.goals[0],
                "position": g.positions[0],
                "load": g.loads[0],
                "hand": g.hand,
                "limp": g.limp,
                "released": g.released,
            })
        })
        .collect();
    eprintln!(
        "third_13_count_control {}",
        serde_json::json!({
            "window_from_us": window.start.as_micros(),
            "window_until_us": window.end.as_micros(),
            "rows": held.len(),
            "false_rows": false_rows.len(),
            "first_false": false_rows.first(),
            "last_false": false_rows.last(),
            "slump_s": report.released[0].seconds,
            "all_rows_match_physical_hold": held.iter().all(|(_, g)| {
                !g.limp && g.released == 0 && g.eased == [1, 0, 0]
                    && g.goals[0] == pinned_at
                    && g.positions[0] == pinned_at + moved && g.loads[0] == 0
            }),
        })
    );
    for (_, g) in held {
        assert!(!g.limp && g.released == 0 && g.eased == [1, 0, 0], "{g:?}");
        assert_eq!(g.goals[0], pinned_at, "{g:?}");
        assert_eq!(g.positions[0], pinned_at + moved, "{g:?}");
        assert_eq!(g.loads[0], 0, "{g:?}");
        assert!(g.hand, "the continuing limp hold disappeared: {g:?}");
    }
}
```
