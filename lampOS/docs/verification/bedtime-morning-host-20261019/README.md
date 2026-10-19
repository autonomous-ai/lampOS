# Bedtime and Morning: host verification, 2026-10-19

Issue #1672. The current rebase uses integration base `6ea2b8ffe`. The full
workspace gate passed before the text-origin guard integration; complete
tests for the ten affected crates and consumers passed on the new source:
**4,237 passed, 0 failed, 20 ignored**, none filtered out.
Native ARM64 must pass on the published head.
Earlier results below apply to their stated revisions. This record covers host software checks for the disabled core
routine and its generic rest and output-limit contracts. See
[behavior and rollout boundaries](../../bedtime-morning.md). The rebase keeps
the merged presence/light-out behavior alongside the disabled routine.

## Behavior and invariants

- Seven core skills are installed, six enabled. Bedtime and Morning remains
  `enabled = false`, with no advertised tool. Its explicit grants cover
  tracked faces, Voice state, body, light, speech and private output limits.
  The three process switches remain off in the product units.
- Authored windows are evening 21:00–23:00, absent-desk rest 23:00–06:00,
  morning 06:00–10:00, and ceilings 21:00–06:00. Fresh observations and
  30 seconds of resting Voice state are required. Daily chances are durably
  consumed before admission and survive restart, reload and clock rollback.
  Missing camera data is never evidence of absence.
- Expression owns leased rest and wake, with no additional actuator writer.
  Manual sleep, privacy, pause, stronger reflexes, changed ownership, stream
  loss and expiry take precedence. Renewals cannot revive a terminal episode.
  Morning waits for correlated planning completion before asking Voice for
  its hum; completion does not prove physical movement.
- Caps combine by minimum, preserve quieter user settings and never unmute
  or restore an old setting on release. Unknown time retains existing caps.
  The ring starts dark until a valid snapshot; publication is not optical
  confirmation. Durable state uses private directories and files.
- Product Skills uses `LAMP_SKILLS_ASK=skip`. Policy mode remains optional
  and may refuse an `unprompted` routine, consuming that daily chance.
  Policy-mode nighttime behavior is not qualified by these checks; no rule
  or reminder exception was added to guarantee output.

## Meaningful regression coverage

These boundaries passed in the complete workspace run recorded below.

| Tests | Boundary exercised |
|---|---|
| [Skills daily engine](../../../crates/skills/tests/daily_engine.rs) | Disabled filtering, fresh Voice/camera evidence, local windows, durable consumption, storage faults and definition fencing. |
| [Skills process fixture](../../../crates/skills/tests/routine_service.rs) | Actual process waits for `completed` before Say; cancellation or a changed Expression owner suppresses the hum; denied body or speech reaches neither owner. |
| [Expression requests](../../../crates/expression/tests/rest_requests.rs) | Session/episode binding, stale or replayed requests, lease loss and stronger owner/reflex boundaries. |
| [Skills limits](../../../crates/skills/src/limits.rs) | Local-time/DST endpoints, exact-token restart recovery, denied cap batches, cleanup after permission revocation and reload fencing at the final write. |
| [Actions caps](../../../crates/actions/src/caps.rs), [service](../../../crates/actions/src/caps_service.rs) | Durable restrictions, retries and unknown-clock recovery; mixer/publication failures cannot claim confirmed readiness. |
| [Ring worker](../../../crates/voice/tests/ring_worker.rs) | Dark startup, minimum ceilings, tightening without another frame and preserved user light-off behavior. |
| [Installer](../../../crates/update/tests/install.rs), [packages](../../../crates/skills/tests/package.rs) | Disabled core inventory and grants, private durable paths, unchanged live catalogue and core removal/replacement protection. |
| [Export](../../../crates/voice-eval/src/export.rs) | All declared topics recognized; rest and cap payloads omitted, with counts retained under their known topic names. |

## Gate evidence

The earlier workspace run preceded permissions integration:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.52 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 26.22 s |
| `cargo test --workspace -- --test-threads=1` | 6,993 passed, 1 failed, 44 ignored; exit 101 | 1,125.49 s |
| Workspace build | Not run after the failed test stage | — |

The failure was
`export::tests::the_bus_topics_hold_every_topic_the_messages_name`, reporting
`output/caps`. The fix recognizes that topic plus `expression/rest_request`
and `expression/rest_state` for omission accounting only. The added
`routine_and_output_cap_bus_payloads_are_omitted_and_counted_by_topic`
regression checks that their payloads never enter exported events or replay
files. No payload admission rule was broadened. Scoped formatting and static
coverage of all 52 named message topics passed before integration.

Permissions-integration preflight also corrected three test fixtures: two
used a manual tool for an automatic-only routine, and one permission
replacement also changed the requested action. A subsequent review found
an in-flight cap read could outlive a definition reload; generation checks
now cover reconciliation and each nonblocking mutation write. Socket waits
and replies do not hold the definition lock. The complete workspace gate below
includes its held-read and write-boundary regressions.

The first full run after that preflight stopped during compilation, before
any test ran, because cached `lamp-msgs` artifacts lacked the new exports.
The source hashes and dependency paths were unchanged and correct. Cleaning
only that package's generated cache resolved the compile failure without
editing source; the complete workspace then compiled in 1 minute 15 seconds.
The subsequent test result belongs below, separately from this diagnosis.

The complete workspace gate on integration base `55271a2de` passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.29 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 18.44 s |
| `cargo test --workspace -- --test-threads=1` | 7,259 passed, 0 failed, 44 ignored; none filtered out | 1,191.15 s |
| `cargo build --workspace` | Passed | 11.87 s |

The formatting and Clippy checks were reused from the successful integrated
preflight only after all 47 source/configuration input hashes matched. The
[workspace record](workspace-before-runtime-integration.json) includes those
hashes, exact commands, log hashes and every Cargo test summary. The earlier
failures remain in [earlier attempts](earlier-attempts.json).

The first rebased standalone run stopped at **56 passed, 1 failed, 0
ignored**. It exposed a shutdown race: a stop request could bypass the next
service tick after a cap storage failure and return success. The fix reads
the sticky failure before normal exit, without more disk, mixer or bus work.
A deterministic clock-boundary regression failed against the old server and
passed with the fix; all **80 Actions tests** and strict Actions Clippy
passed. The original failed-exit assertion remains. The regression also
checks that the restriction snapshot and mixer writes are unchanged.

The source-only experiment also revealed an inherited packaging-test
boundary: Update's `tests/image_journal.rs` reads the parent repository's
`robots/lamp/rootfs` image drop-in. The final gate therefore runs **all Update
tests in the original checkout**, and the other **41 complete crate suites
plus the workspace build in the source-only copy**. No test is filtered or
silently skipped to bypass the missing fixture. This limits the claim of
whole-workspace standalone test portability; it is not a runtime dependency
introduced by this change. [Consumer coverage](consumer-coverage.json)
records the eight changed crates and 27-crate reverse-dependency chain.

The previous gate on integration base `d9ba8d0f2` passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.44 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 17.26 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,172 passed, 0 failed, 43 ignored | 1,135.80 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 124 passed, 0 failed, 1 ignored | 89.72 s |
| `cargo build --workspace` | Passed | 22.59 s |

Combined: **7,296 passed, 0 failed, 44 ignored**, none filtered out. The
[pre-rebase record](before-light-out-rebase.json) includes every Cargo summary, per-crate counts,
command/log hashes, all 47 changed source/configuration hashes and the full
1,481-file source-copy digest. All source hashes still matched at completion.
The standalone-test limitation above is explicit; the Update suite was run
whole in its original checkout. Native Linux ARM64 CI must pass for the
published PR head before it is ready.

The previous complete gate on integration base `89bd0e65f` passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.64 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 46.48 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,214 passed, 0 failed, 43 ignored | 1,221.43 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 124 passed, 0 failed, 1 ignored | 70.35 s |
| `cargo build --workspace` | Passed | 16.80 s |

Combined: **7,338 passed, 0 failed, 44 ignored**, none filtered out, across
all 42 crates. The [pre-scenes gate record](before-light-scenes-rebase.json) includes every Cargo
summary, per-crate counts, command/log hashes, 47 changed source/configuration
hashes and the full 1,507-file source-copy digest. All source hashes matched
at completion. No source changed after the gate began. This run includes the
light-out/presence rebase and preserves the earlier results as history.

## Light-scene integration and text-origin guard

The complete workspace gate for `b7dbae26f`, based on `4bcf71bd8`, passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.32 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 47.26 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,227 passed, 0 failed, 43 ignored | 1,248.33 s |
| `cargo test -p lamp-update -- --test-threads=1` | 125 passed, 0 failed, 1 ignored | 62.40 s |
| `cargo build --workspace` | Passed | 17.48 s |

Combined: **7,352 passed, 0 failed, 44 ignored**, none filtered out, across
all 42 crates. [The full record](before-text-guard-rebase.json) pins every
Cargo summary, command/log hash, 47 changed inputs and the complete
1,539-file source-copy digest. Every source hash matched at completion.

The subsequent rebase preserves the upstream text-only task results and
refuses body-granted tools from typed requests. Its catalog emits body
metadata only for enabled, admitted tools. Actual Bedtime remains disabled
and has no advertised tool. The complete follow-up suites cover Body, Cloud,
Expression, Keeper, Lean, Memory, Skills, Update, Voice and Voice-eval: every
changed crate since the full gate and every reverse consumer. Workspace
formatting, strict all-target Clippy and the workspace build are repeated.
The follow-up passed **4,237 passed, 0 failed, 20 ignored**, none filtered out.
[Its gate record](gates.json) contains the exact commands, every Cargo summary,
47 changed input hashes and the complete 1,541-file source-copy digest.
[The source comparison](text-integration-coverage.json) proves that all
changed runtime/test source since the full gate belongs to those three crates,
and that their ten-crate consumer closure was tested completely. The only
other non-`docs/` changes are two builder documents. Every frozen source hash
matched at completion. Counts from the two runs are separate, not added.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.58 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 54.99 s |
| `cargo test -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-skills -p lamp-voice -p lamp-voice-eval -- --test-threads=1` | 4,112 passed, 0 failed, 19 ignored | 733.47 s |
| `cargo test -p lamp-update -- --test-threads=1` | 125 passed, 0 failed, 1 ignored | 65.33 s |
| `cargo build --workspace` | Passed | 36.41 s |


## Latency and failure boundaries

The fake-sink cap-tightening regression bounds bus publication to a black
frame below 150 ms, before the old permit expires. This is a host software
boundary, not a lamp optical measurement. No new physical latency or
first-use/repeated-use distribution is established here.

Designed cadences are 20 ms Expression planning, 200 ms planning receipts,
one-second rest renewals with leases of at most three seconds, and original
request age below 500 ms. The ring intake checks at most every 20 ms off its
output loop. Morning waits at most 12 seconds for planning completion and
retains its original admission deadline through Say. Timeout, cancellation
or a changed generation prevents delayed presentation.

Daily journal sync precedes admission and can delay it. Cap socket and mixer
work runs outside Skills observation. Mixer read, write and readback each
retain an 800 ms timeout: about 2.4 seconds plus process overhead is a failure
budget, not a measured application latency. Actions retries required clamps
and publication on its service cycle, at most once per second; failed
confirmation withholds readiness.

The previous head `48a475a0f` passed native Linux ARM64 in
[run 39198121073](https://github.com/autonomous-ai/lampOS/actions/runs/39198121073):
7,380 passed (1 leaky), 48 skipped, plus four doc tests and twelve own-harness
tests, all passing. The reported leak was
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. That
result does not validate the rebased head; its separate run is still required.

Host fixtures do not establish systemd-created directories, installed-service startup,
real motor/fold behavior, physical privacy response, optical darkness,
mixer hardware confirmation or the hum's sound and onset. The coordinated
lamp comparison in the behavior document remains required before enabling
the core skill. No device deployment or access is part of this verification.

The published head `d27632ab6` passed native Linux ARM64 in
[run 39203781644](https://github.com/autonomous-ai/lampOS/actions/runs/39203781644):
**7,423 passed (1 leaky), 48 skipped**, plus **4 doc tests** and **12 own-harness
tests**, all passing. The same existing leak was reported. Release build,
checksums and artifact upload succeeded. [Its native record](before-light-scenes-arm64.json)
pins the exact head and log hash; it predates the light-scenes rebase and does
not validate the new head.
