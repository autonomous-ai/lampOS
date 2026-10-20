# Bedtime and Morning: host verification, 2026-10-19

Issue #1672. Publication source `0e8817fd6cfc6efbcd62c6b93c86a0d48c66db30`
is based on #1712's `b919291458c3618838c5f4b56520cf3b2cd739a5` and
contains only Bedtime's task delta. The inherited Tasks diagnostic race is
split into test-only PR #1966; it is absent here.

The machine-readable run records this README names (gate records,
preflights, consumer coverage, earlier attempts) are not committed; this
README is what is kept of them, and the commands below can be run again.
These records are of the #1712-based source; this copy carries the same
change rebased onto lamp-voice.

The fresh publication preflight passed:

| Command or scope | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.74 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 125.64 s |
| Four named second-route Voice cases | Each 1 passed, 0 failed, 0 ignored; 2,261 filtered | 61.14 / 0.29 / 0.57 / 0.48 s |
| `cargo test -p lamp-skills -- --test-threads=1` | 220 passed, 0 failed, 0 ignored; none filtered | 82.82 s |
| `cargo build --workspace` | Passed | 77.38 s |

All 1,592 non-document source hashes still matched after the source-only
checks. Publication changes only verification documents after that freeze.
The identifier and reverse-consumer scan covers
nine changed crates and their 27-crate closure. Its full test coverage is
pending in the PR's native ARM64 run, the gate of record under the first
builder's 22:04 direction on #1722. The PR remains draft until that run is
green. These focused counts are not a full host workspace gate.

The coordinator retains the base's cue/Expression fixture and all 26 base
tests beside the three new Bedtime cases. Exact optional words use the
second route's written turn, with Schedule reminders both off and on. The
authored morning hum uses local PCM and no provider input. Original expiry
and the unchanged quiet gate still prevent stale or early output. Existing
Skills-process tests exercise owned Wake and denied permissions with local
peers. These checks do not establish a physical-speaker or provider result.

The prior second-route preflight qualifies
`08d9177cb4` only. Its full attempt
stopped at the unchanged Tasks race and is not a passing full gate. The
sandbox-denied IPC attempt is also
retained separately. Historical counts below name their own revisions.

## Historical qualification records

Issue #1672. The earlier preparation ported the completed Bedtime P3 work
onto #1712's second-route head `5aff09571944fa81deeb11d3901a7bdba2bc8ed0`.
At that checkpoint the second-route tests and replacement gates were pending. The three Skills conflicts are resolved as the Bedtime
change only. Nudge/Search and held-colour features from the earlier parent
are absent from this target, so they are not imported as conflict context.
The daily integration test uses the target's existing held dim to retain
clock, reload, restart and next-day checks. The preceding combined budget
and held-colour result remains historical evidence for its own source.

The completed P3 host gate on frozen
`6d364e3af791702ba40a5bd0d059c79ed2bd6651` is preserved in
the preceding record: **7,651 passed,
0 failed, 44 ignored**, none filtered, across **43 crates**, plus **12
separate harness tests**. All **1,629** non-document source files and **156**
non-evidence documents matched. That result does not qualify this new port.

New tests use the actual second-provider engine with fake chat and speech,
and the production coordinator with stepped process peers. They exercise
Say's exact words with the reminder flag both ways, the authored local hum,
original-deadline expiry and the unchanged quiet gate. Existing real Skills
process tests cover the earlier owned-wake completion and denied-permission
boundaries, with stand-in Expression and Say services. This is not one
end-to-end Skills-to-physical-speaker run and no provider service is contacted.

The preceding full host gate passed on frozen source
`5e83af10210cb1a4bfbeb15887274d1e519fe4c8`: **7,608 passed, 0 failed,
44 ignored**, none filtered, across **43 crates**, plus **12 separate
own-harness tests**. Published `805f56961` then passed
native ARM64 39239473411: **7,693 nextest passed
(1 leaky), 48 skipped**, plus **4 doc and 12 separate harness tests**, on
merge checkout `cbea9ad9d` against `179016adc`. Those counts qualify only
that earlier source, not the new rebase or follow-ups.
Native ARM64 must qualify each published PR head separately; PR runs test its GitHub merge commit. Earlier results
below belong only to their stated revisions. No device was accessed.

The first P3 attempt on `49d1ebecb` passed formatting (**3.60 s**) and
strict workspace/all-target Clippy (**55.93 s**), then was stopped during
tests after static review found that a delayed wall-clock sample could be
anchored to a later processing time. The interrupted record
preserves those completed stages and the partial-test log hash; it is not
a complete passing gate. The replacement retains the original host-monotonic
sample time and checks delivery delay at the window boundary.

## Completed P3 host gate

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.50 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 23.53 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,517 passed, 0 failed, 43 ignored | 1,251.19 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 134 passed, 0 failed, 1 ignored | 73.40 s |
| `cargo build --workspace` | Passed | 17.77 s |

The exact preceding P3 record contains every summary, command and log
hash, all 51 changed inputs, and the complete 1,629-file non-document source
digest. All frozen source matched at completion. All 42 non-Update suites
and the build used a source-only copy; the complete Update suite used the
checkout for its inherited parent image fixtures, without test filtering.
This result qualifies the stated source only. It does not qualify the
subsequent second-route integration or physical speech on a lamp.

## Earlier complete review-fix gate

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.54 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 51.20 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,474 passed, 0 failed, 43 ignored | 1,260.35 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 134 passed, 0 failed, 1 ignored | 77.36 s |
| `cargo build --workspace` | Passed | 16.41 s |

The preceding complete gate record preserves every Rust summary,
per-crate counts, commands, log hashes, 51 changed inputs and the full
1,617-file non-document source digest. All frozen source and non-evidence
documents matched at completion. The publication amendment updates only
verification records and this README. The identifier scan
covers nine changed crates and their complete 27-crate reverse-consumer
closure, all included in the gate.

All 42 non-Update suites and the workspace build ran in the source-only copy.
The complete Update suite ran in the checkout because its inherited image
tests read parent-repository fixtures; no filter or added ignore bypassed
them. The merged nudge-limit and held-color changes are retained, with a
combined budget/daily-claim/clock-trust regression. The real Skills process
test retains an admitted rest episode through an unrelated folder reload.

## Review fixes and preflight

The matching Actions/Skills build must be published to the selected channel
before installing units containing the new flags. Both product-reset plans
now wipe the two new durable stores and retain private service directories.
All three bus topics are registered and documented; additive fields are
accepted while unknown operations/phases and invalid bounds remain refused.

The daily journal now records exact dates under schema 2, bounded to 128
keys and 1 MiB. Correcting a mistaken future date does not consume unused
intervening dates. Legacy schema 1 migrates conservatively, because it
already discarded exact history. Scheduled daily/rest/cap work requires
kernel synchronization or the explicit development `--trust-clock` override;
ordinary quiet hours retain their existing semantics.

The sender preserves an admitted rest through a short stale observation,
only until the already-issued three-second lease ends. Renewal requires
fresh matching owner and episode evidence. An unrelated definition reload
preserves admitted rest only when its complete enabled owner is unchanged;
pending transitions, wake runs and old presentation permits are invalidated.
Independent review found no remaining P1/P2 in that focused scope.

The first preflight stopped on a
Clippy style warning. The second
passed Clippy but its new fixture incorrectly exposed an automatic-only rest
step as a model tool; 13 unit tests were rejected during fixture loading.
The corrected complete Skills/Msgs/Setup preflight
passed **410 tests, 0 failed, 0 ignored**, none filtered, in **82.33 s**;
formatting **3.59 s** and strict all-target Clippy **8.82 s** also passed.
Those results qualify pre-rebase commit `c2dea42988205689e00557a72405e53bffae9aaf`.

The old published head `1d9112d166` passed native ARM64
with **7,597 passed (1 leaky), 48 skipped**, plus 4 doc and 12 separate
harness tests. That result does not qualify these new review fixes.

The review's smaller documentation findings are corrected: stream-gap
qualification, bounded restart history, global journal-fault/key behavior,
output-limit permissions and non-restored volume, switch PR provenance,
installed exclusions and the actual scope of the stand-in test fixtures.
The ring remains dark before its first authoritative cap snapshot, and cap
tightening still blanks before the next repaint (up to 25 ms). A delayed
snapshot diagnostic and an eased transition remain follow-ups; no lamp
observation of either is claimed. Synchronous daily durability remains at
most three claims per day for this authored skill. The final lamp comparison
and merge remain with First.

## Earlier rise-trigger and exact-speech integration

The pre-review source was based on `b03cb4a4b3`.

The new integration preserves the merged rise triggers, exact/ranked speech,
text-origin body guard, sound classes and ReportsStop route. Bedtime's
original Wake deadline and last-moment permissions remain authoritative.
Daily claims, windows and rise state revalidate together across reloads.
The process regression carries exact priority-8 speech with a one-hour
expiry through an owned Wake, retaining the original start deadline. Denied
body or speech reaches neither Expression nor Say. A new daily-rise fixture
initially lacked private directory permissions; the failed 187/1/0 preflight
and its fixture-only correction are recorded in the earlier-attempts run record. The
complete corrected Skills preflight passed 188/0/0 before this gate.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.50 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 54.74 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,293 passed, 0 failed, 43 ignored | 1,300.23 s |
| `cargo test -p lamp-update -- --test-threads=1 (checkout)` | 130 passed, 0 failed, 1 ignored | 48.76 s |
| `cargo build --workspace` | Passed | 18.10 s |

Combined: **7,423 passed, 0 failed, 44 ignored**, none filtered out, across all 42 crates. The pre-review gate record contains every Cargo summary, per-crate counts, command/log hashes, 47 changed inputs and the complete 1,549-file non-document source digest. All frozen source hashes and all 155 non-evidence documentation hashes matched the tested source-only copy when this full gate completed. The later installation-document and test-fixture relocations are qualified separately below. Consumer coverage records the changed identifiers and complete 27-crate reverse-consumer chain.

All 41 non-Update suites and the workspace build run from the source-only copy. The entire Update suite runs in the checkout because its inherited image-journal and image-ramlog tests read the parent image fixture. No test filter or added ignore bypasses that limitation. Earlier local and native results remain in the prior local record and prior native record.

## Installation-document merge follow-up

The subsequent integration head `da0806945e` adds Not feeling well beside
an existing installation list. Moving the existing Light scenes paragraph
out of that overlap resolves the conflict while preserving its meaning;
no runtime source changed. The identifier grep found only Update reads
`install.md` as test data; other references are comments or help text.

Formatting passed (3.63 s), strict all-target Update Clippy passed (1.50 s),
and the complete `cargo test -p lamp-update -- --test-threads=1` passed
**130 passed, 0 failed, 1 ignored**, none filtered, in 45.21 s. This follow-up
is separate from the 7,423-test workspace result. The pre-review gate record
keeps both document hashes, every follow-up command/log hash and output
summary. At this follow-up, all 1,549 frozen non-document source hashes and the other
154 non-evidence documents still matched the full-gate snapshot.

## Export-fixture merge follow-up

Merged VoiceTurn adds a sample beside this task's three routine/cap samples.
Moving those same three records to the end of the fixture avoids the overlap.
The whole production prefix of `voice-eval/src/export.rs` is byte-identical,
and the other 1,548 non-document source files still match the full gate.
The identifier scan finds no consumer outside Voice-eval; the dependency
graph has no reverse consumer of that crate.

Formatting passed in **4.22 s**, strict all-target workspace Clippy in
**26.33 s**, and the complete Voice-eval suite passed **540 passed, 0 failed,
0 ignored**, none filtered, in **125.36 s**. These rechecked tests are
separate from the 7,423-test full gate. `source_followup` in the pre-review
record keeps both file hashes, the unchanged production-prefix hash,
remaining-source proof, exact commands and every test summary.

The preceding published head `c7f9c90879` passed ARM64
[39221703897](https://github.com/autonomous-ai/lampOS/actions/runs/39221703897):
**7,520 passed (1 leaky), 48 skipped**, plus **4 doc tests** and **12 own-harness
tests**. The relay harness, release build, checksums and artifact upload
passed. That record identifies the PR
head and tested GitHub merge `9eeb73f0a0` against `da0806945e`; it does not
qualify the new fixture revision. The replacement PR head needs its own
native run. The inherited leak and output-collision warnings remain recorded.

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
| [Skills process fixture](../../../crates/skills/tests/routine_service.rs) | Actual process waits for `completed` before Say; cancellation or a changed Expression owner suppresses presentation; denied body or speech reaches neither owner. Exact priority-8 speech keeps its authored one-hour expiry and the original Wake start deadline. |
| [Combined Skills triggers](../../../crates/skills/tests/skills.rs) | Daily rise-trigger chances stay consumed across reload/restart; pending replies must retain the current rise, local window and definition generation. Exact speech and a routine cue share one presentation slot. |
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
workspace record includes those
hashes, exact commands, log hashes and every Cargo test summary. The earlier
failures remain in earlier attempts.

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
introduced by this change. Consumer coverage
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
pre-rebase record includes every Cargo summary, per-crate counts,
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
all 42 crates. The pre-scenes gate record includes every Cargo
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
all 42 crates. The full record pins every
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
Its historical gate record contains the exact commands, every Cargo summary,
47 changed input hashes and the complete 1,541-file source-copy digest.
The source comparison proves that all
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
checksums and artifact upload succeeded. Its native record
pins the exact head and log hash; it predates the light-scenes rebase and does
not validate the new head.
