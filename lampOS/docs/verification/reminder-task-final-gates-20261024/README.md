# Current reminder/task fixture gates

Second, 24 October 2026, 10:12 +07. PR #3312 head
`4ba9d959ff8e3da495e699dc9edf6cdbf6e6d528`, containing `9ed44f88c`.
No product runtime code changes are in this PR.

Mac Rust 1.96.0: `cargo fmt --all --check` and
`cargo clippy -q -p lamp-schedule -p lamp-tasks -p lamp-voice --all-targets -- -D warnings`
passed. Tests were run by crate/target, with no overlapping Cargo process:

| Command after `cargo test -q` | Passed | Failed | Ignored |
|---|---:|---:|---:|
| `-p lamp-schedule -p lamp-tasks` | 434 | 0 | 11 |
| `-p lamp-voice --lib -- --test-threads=4` | 3,696 | 0 | 15 |
| `-p lamp-voice --bin lamp-voice` | 1 | 0 | 0 |
| `-p lamp-voice --test it --test release_pair -- --test-threads=1` | 269 | 1 | 0 |
| Isolated exact failed integration case, below | 1 | 0 | 0 |
| `-p lamp-voice --test release_pair` | 0 | 0 | 6 |
| `-p lamp-voice --examples` | 10 | 0 | 0 |
| `-p lamp-voice --doc` | 0 | 0 | 0 |

Unique completed successful coverage totals **4,411 passed / 32 ignored**;
this is **not one uninterrupted all-green command**. The integration attempt
failed at `ring_worker::output_caps_clamp_turn_frames_and_tightening_blanks_latched_pixels_without_a_new_frame`,
`ring_worker.rs:227`, with `StaleOwner` and the worker's authority-expired
message. The same exact case then passed alone in 0.14 seconds:

```sh
cargo test -q -p lamp-voice --test it ring_worker::output_caps_clamp_turn_frames_and_tightening_blanks_latched_pixels_without_a_new_frame -- --exact --test-threads=1
```

The complete `crates/voice/src` tree and that integration file match the
`9ed44f88c` base (`git diff --exit-code origin/lamp-voice HEAD -- lampOS/crates/voice/src lampOS/crates/voice/tests/it/ring_worker.rs`, from repository root).
No production/test change was made to hide that timing-sensitive failure.
The failed attempt and isolated retry are both retained. Remaining manual,
example and doctest targets ran after the retry; the whole suite was not repeated.

Exact-head Linux source-only formatting passed with **6,977 regular files
unchanged**, a clean worktree and verified owned-container cleanup. The
pinned container was offline/read-only, UID/GID 65534, with no host mounts or
devices. It did not compile or test ARM64 locally.

Final native runs are separate: published pairs 40057006999, ordinary ARM64
40057006981 and strict Linux Clippy 40057007027. Their results were pending
when this record was written. Earlier successful/failing heads remain separate.
