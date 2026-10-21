# Unplug preparation host gate, 19 October 2026 (#1674)

Ready for lamp-voice: no. Native ARM64 CI and the authorized real-lamp check
remain outstanding. The code stays off unless `LAMP_UNPLUG=on`.

Base commit: `6275fcc8cb2c229f8b18a6db3b4485cb46a11676`. `source-sha256.txt` pins the changed implementation
and tests validated here. This is Mac host evidence, not a lamp result.

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| lamp-body | 464 | 0 | 0 |
| lamp-cloud | 36 | 0 | 0 |
| lamp-expression | 272 | 0 | 0 |
| lamp-keeper | 24 | 0 | 0 |
| lamp-lean | 61 | 0 | 2 |
| lamp-memory | 209 | 0 | 5 |
| lamp-skills | 93 | 0 | 0 |
| lamp-update | 118 | 0 | 1 |
| lamp-voice | 2179 | 0 | 11 |
| lamp-voice-eval | 539 | 0 | 0 |
| Total | 3995 | 0 | 19 |

Commands, from `lampOS/`, with `RUSTC_WRAPPER=` (the sandbox refuses sccache):

- `cargo fmt --all --check`.
- `cargo clippy -p lamp-body -p lamp-expression -p lamp-skills -p lamp-voice -p lamp-update --all-targets -- -D warnings`.
- `cargo clippy -p lamp-cloud -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-voice-eval --all-targets -- -D warnings`.
- `cargo test -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval -- --test-threads=1`.
- After the fixture correction below, the same test command continued with
  Expression through Voice-eval; Body and Cloud had already passed. The table
  counts each successful suite once.
- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build --workspace`.
  Debug info was disabled only for this final build after cleaning this
  checkout's over-20-GB target, per the Queue's disk rule.
- Final `cargo clippy -p lamp-expression --all-targets -- -D warnings` after
  the fixture correction, with the same small dev profile, passed too.
- The body was also checked by `cargo clippy -p lamp-body --target
  aarch64-unknown-linux-gnu --all-targets -- -D warnings` (includes the actual
  Linux body command wiring). The full ARM64 workspace still belongs to CI.

## Failures retained

The initial sandbox Body lib run could not open IPC endpoints (EPERM); outside
that sandbox its full library passed. The first combined gate failed three
new Expression runtime fixtures because macOS's long temporary-directory prefix
made the Unix socket path 108 bytes, over its 103-byte limit. The fixtures now
use a short unique `/tmp` root; all three pass, then the full remaining gate
passes. No test assertion or production limit was disabled.

The one-thread host gate follows the known macOS fixture constraints. It is
not proof that the repository's pre-existing global-flag tests are parallel-safe
(#1691/#1692). The 19 ignored tests remain ignored; no physical test was counted.

## Latency and completion boundaries

A separate run of the already-built `unplug_tool_returns_before_folding_and_says_ready_only_after_render_receipt`
process fixture measured **230.667 microseconds** for one warmed Mac tool call:
from writing the call on the skills socket to reading its JSON admission. The
admission is explicitly `ready:false`. This does not measure speech recognition,
model choice, motor travel, camera work, first boot or real-lamp latency.

The fixture verifies no readiness speech before body, Expression and an actual
ring receipt are present. Ring tests use a recording sink on both resting and
Speaking paths; permission failure and failed writes give no acknowledgement.
Voice tests invalidate proof with delayed audio queued and verify no speaker
packet, normal cancellation and rejection of the old output permit. Body tests
exercise the existing guarded trajectory, blocked joints, filtered bad reads,
startup/recovery interruption and immutable calibrated fold targets.

The real lamp must still establish physical support after folding, the chosen
12-count/500-ms tolerances, first/repeated timing, privacy and restart behavior,
and the visible and acoustic confirmation. No power was cut for this evidence.
