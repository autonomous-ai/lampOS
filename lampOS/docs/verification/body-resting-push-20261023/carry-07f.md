# Body carry control read, 23 October 2026

Exact head: `07f5cc66501b3f6b0b22ef555f20efa0c7f75ad6`. Intel macOS, jobs 2, offline and locked.
The detached worktree was clean before and after; no fixtures or product code changed.

| Control | Exit | Result |
|---|---:|---|
| `a_sustained_gentle_push_is_said_until_the_hand_lets_go` | 0 | pass |
| `a_pitch_contact_during_glance_cooling_is_said_as_a_hand_before_it_slumps` | 0 | pass |
| `stationary_pressure_clears_within_one_tick_after_load_and_displacement_clear` | 0 | pass |
| `stationary_pressure_remains_a_hand_when_it_eases_above_half_the_bound` | 101 | fail: 95/95 late rows report no hand |
| `a_pinned_pitch_held_thirteen_counts_away_still_says_a_hand_before_slump` | 0 | pass |
| `a_resting_joint_jittering_inside_its_band_never_says_a_hand` | 0 | pass |

Each control ran with:

```text
CARGO_BUILD_JOBS=2 cargo test -q --offline --locked -p lamp-body --lib <control> -- --test-threads=2
```

The stationary 300-to-130 test fails at steps 250–344, goal and position
both 2000, load 130. The retained-hand predicate at `body.rs:6099` accepts
this load, but the stationary `held_from` branch at `body.rs:6210` requires
the full 240 bound to renew easing. When the hold expires, the publication
gate suppresses the hand and the predicate clears it. The carry combines
`f403f4dd7` with landed `2f0ced485`; the independent ARM64 run
`39929628390` fails the same test. The HOLD remains. Hold renewal must
preserve an admitted hand without admitting untouched resting weight.

This is simulated control evidence, not motor safety or a device trial.
No ARM64 linking or lamp access was performed.
