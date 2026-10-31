# Tiers work log

## 2026-10-31: reviewed adapter and pre-score coverage audit

Recall/adapter code is ready for review at `38a822541` in
[PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756), now undrafted.
Native ARM64 run 41200748151 passed 13,003 tests (107 skipped) plus doctests;
strict Linux clippy run 41200748106 passed. All 65 host functional tests,
three API tests, formatting, clippy, workspace build and 18 source-only
subprocess tests passed. Tests recorded READ-OK; the privacy re-read is pending.
The final change strengthens the test harness: empty privacy results must have
zero deadline misses, expected exits have a bound, stderr is preserved, and the
broker timeout is measured. Engine sources match the verified month at
`444ac6b66`. The pinned macOS executable and checksum were shared for reuse.

The structure-only audit at registration `3d8be60e8` covered all fourteen
setups without inspecting question or answer text. The 29.32-day event stream
cannot exercise the 90-day fading rule. Query topic, activity and object cues
were absent, and only one observed person identity was present. The recorded
candidate held 240 closed episodes, seven explicit statement facts and no
modeled lines; successful plumbing does not establish modeled-tier value.
Its 1 MiB allowance did not stress the observed 2,085-byte memory text.

Before any score, the lead ruled: keep the month and fading threshold, register
a separate aging test through about 120 days, add the product's seat-based
activity signal, use a 4,096-byte memory allowance except the full-history
ceiling, and mark empty or unused counterparts as not exercised. No topic or
object labels are to be added. A second known person remains a declared gap.
Per-tier inventory and recall/snapshot use are optional count-only contract
2.1 diagnostics, never prompts or scores; proof-only fact rows are separate.

That next concern is on `tiers/usage-diagnostics`, based on the green head.
The first 76 functional tests and three API tests passed; its final gates are
not complete. The shared Mac now has one build slot, acquired atomically at
`/Users/d/harnesses/worktrees/autonomous-os/.lamp-build-slot`; require 15 GiB
free after acquiring it. Preserve pinned executables, clean only the owner's
build output, and release the slot after each checkpoint. Live scores and the
complete current-product baseline remain unfulfilled acceptance evidence.

## 2026-10-31: owner departure and regenerated-month evidence

Engine `444ac6b66` in [PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756)
passes native ARM64 run 41198582162 (13,002 tests, 107 skipped, plus doctests)
and strict Linux clippy run 41198582167. Host formatting, all-target clippy,
64 functional tests plus three API compile-fail tests, workspace build and
the source-only suite passed. The departure guard now requires the same owner
identity as capture. Its paced regression failed before the correction and
passes after it. No product caller or device changed.

The full candidate completed the regenerated month with runner and registration
`3d8be60e8`: 2,220 events, 200 questions, 224 answer turns, 30 nightly passes and
four weekly passes. All eight pending weekly batches drained and all 200 probe
restorations completed. Snapshot and recall deadline misses were both zero.
The ledger verifies as completed with 4,536 entries and final hash
`e8ec1921da76e4aaafaec55433f632d27b83f4bdfb16d1d0aea0062b0b19cfb3`.
The lead received its summary and manifest/events bundle, excluding the private
database and key. Fixed replies and empty consolidation made 266 recorded calls,
zero live calls, with no transcription or scores. Hidden answers were not read.

Ready for the whole assignment: no. The code has the native gates; review
follow-up tightens empty-result deadline assertions and bounds child exit waits
before scoring. Real model/STT access, the complete product baseline, and the
registered eleven-arm three-run scorecard remain outstanding. Most likely wrong
remains semantic forgetting and claims that overstate their evidence.

The shared-machine disk rule is now explicit: retain pinned executables and
ledgers, use one build directory, require 15 GiB free before long work, and clean
rebuildable output after each pushed checkpoint. Cleaning this worktree removed
31.5 GiB of Cargo output and left 25 GiB free; all run evidence was retained.

## 2026-10-31: ordinary adapter target and runner integration

Store/sleep ready for review: yes at `1c9d4cd17` in
[PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750). Native ARM64
run 41194104866 passed 12,972 tests, including all 35 crate tests, and Linux
clippy passed. Reviewer-three recorded READ-OK; the other read is pending.

Recall/adapter ready for promotion: no, exact-head native gates and readers
are still pending at `66b96bb73` in
[PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756). The adapter is
now an ordinary binary target. Under the first builder's explicit narrow
permission, its name alone was added to the development inventory, which is
never packaged. No other deployment entry or device changed.

All 63 functional tests and three compile-fail API tests pass with default
features on macOS x86_64 Rust 1.96.0 and in a source-only copy. Formatting,
strict all-target clippy, workspace build and classification of all 49 current
workspace lamp binaries pass. A UTF-8 exact-cap regression failed at 253 bytes
versus 251 before removing snapshot separators that were never returned;
it now passes. The 500-byte subprocess case also distinguishes returned text
from serialized framing. No public library copy/restore constructor exists.

Runner integration against Bench `2885ca9f3` passes: all eleven arms pass
its public smoke (132 operations, zero model calls and zero memory deadline
misses). A separate synthetic fixture passes two brokered sleep calls across
probe restoration with both receipts retained: 400 microdollars at the test
rate. Its 500-byte request returns 451 memory-text bytes and reports 212
framing bytes separately, 663 rendered bytes total. These are deterministic
protocol/accounting checks; no live invoice, persona score or hidden-answer
inspection is involved.

The reviewer accepted the private-child forgetting semantics; scored
forgetting-persistence cases must occur in the main session, as Bench owns.
The live runner, real STT/model access, full baselines and eleven-arm three-run
ledger scorecard remain dependencies for the overall assignment. Most likely
wrong remains semantic forgetting and model overstatement, to be measured.

## 2026-10-31: calendar reflection and private benchmark probes

Ready for lamp-voice: no; both implementation pull requests need their current
head gates/readers before promotion.

Store [PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750) is at
`1c9d4cd17`. Weekly reflection now looks back seven calendar days in the
owner's named time zone, with spring/fall DST regressions. All 35 store/model
tests, strict host clippy, formatting, workspace build and source-only tests
pass. Linux clippy is green; native ARM64 is still running on this correction.
The two readers were told the exact small change from their previous head.

Adapter [PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756) is at
`e2e74a15e` and includes that calendar fix. Broker/storage failures stop the
run, and isolated probe copies now live only in the development executable.
The library has no public copy, restore or connection-import entry point.
Private child files are removed before resuming the original store; cleanup
failure ends the process without overwriting the main database. Three API
compile-fail tests pass; the fork test was also shown to fail with the former
public API restored. A read-only peer audit found no blocking issue.

On macOS x86_64 Rust 1.96.0, all 62 functional tests plus three compile-fail
tests pass, including source-only checks outside the repository. Formatting,
strict clippy with default and bench features, and workspace build pass.
The benchmark builder independently passed a twelve-operation v2 smoke on
`df9af5447`, with no model calls, scores or deadline misses. This validates
protocol integration only. Current native feature coverage, live model/STT
runs and the baseline plus eleven-arm three-run ledger scorecards remain open.

The lead chose ordinary development binary targets for existing CI coverage.
The shared development-only inventory request includes both lamp-memory-bench
and lamp-memory-tiers-arm; deployment edits belong to its owner. Until that
entry lands, the adapter retains its feature requirement. No product crate,
deployment or lamp was changed. Live relay access remains a live-run dependency,
while independent implementation work continues.

Most likely wrong remains semantic forgetting/paraphrase and model overstatement;
the pending bench must measure them instead of treating host tests as scores.

## 2026-10-31: store review and benchmark v2 checkpoint

Ready for lamp-voice: yes ([PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750), store/sleep only).

Store head `c7d231047` passed strict Linux clippy and native Linux ARM64
[run 41191103824](https://github.com/autonomous-ai/lampOS/actions/runs/41191103824),
including all 34 crate tests. The privacy read is approved; the separate memory
read is requested. The pre-read fixes drop uncertain-speaker input, use a
separate 0600 HMAC key for forget fingerprints, and remove erased episode
rows/times. Host formatting, clippy, tests, workspace build and source-only
verification passed. This remains isolated experimental code with no product
consumer or device side effect.

Ready for lamp-voice: no ([PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756), recall/adapter draft).

Adapter head `df9af5447` implements cue recall and the eleven registered arms,
with a default-off benchmark binary and in-memory probe children. It targets
contract v2 at `5cfcb7455`: actual forget effects and confirmation suppression,
cache invalidation after mutations, and no silent upgrade of v1 results.
All 59 tests pass on macOS x86_64, Rust 1.96.0, and from a source-only copy.
Strict feature clippy, formatting and workspace build pass. A 3000-record
in-memory fixture measured 1.599 ms for first lookup after ingestion and a
1.426 ms maximum over 20 repeats; this is not cold-disk or device latency.

The shared default-feature CI is running. Explicit ARM64/Linux clippy coverage
of `--features bench` was requested from the workflow owner; the test reviewer
offered a separate feature check. The benchmark builder has the revision for
runner smoke integration. No live model/STT or scored baseline/ablation run
has occurred here, and no device was used. Live access and the frozen runner's
three-run ledger scorecard remain dependencies for the overall assignment.

Most likely wrong here: lexical forget recognition can miss paraphrases or
remove too much; model-written claims can overstate their cited words. The
bench must measure those limits, and a unit test is not an accuracy score.


## 2026-10-31: #4746, isolated store and validated sleep

Ready for lamp-voice: no ([PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750), draft).

Revision `eecb9cd36`, based on `d06603109`; branch `tiers/store-sleep`.
The first builder accepted the role/scope and assigned #4746 then #4747 on
[Queue #4710](https://github.com/autonomous-ai/lampOS/issues/4710#issuecomment-6541830902).

Built the isolated SQLite record, episodes, quoted facts and typed memory lines;
bounded nightly/weekly consolidation, cumulative owner-model rewrite limits,
source-based expiry, forgetting fingerprints and invalidation/cancellation;
and one model client with timeout, response and run-cost bounds. No product
Memory, Voice, deployment or device changed. This is an experiment until the
bench scores, readers and owner's decision.

On macOS with Rust 1.96.0:
- `cargo fmt --all --check`: passed.
- `cargo clippy -q -p lamp-memory-tiers --all-targets -- -D warnings`: passed.
- `cargo test -q -p lamp-memory-tiers`: 32 passed, zero failed.
- `cargo build -q --workspace`: passed, including after rebase.
- Source-only copy of the crate plus `lamp-local-time` outside the parent
  repository: the same 32 tests passed.
- `git diff --check`: passed.

Evidence: `crates/memory-tiers/tests/` and `docs/memory-tiers.md` on the task
branch. Synthetic JSON fixtures are not live relay captures. ARM64 and Linux
clippy CI run on the draft PR; no result claimed yet. Live model access needs
an authorized private config/key path. Cue recall, natural-language forgetting,
the adapter, ablation scores and device latency are not shown by these tests.

Most likely wrong here: a model paraphrase can overstate a source despite the
preserved strength label; lexical forgetting cannot recognize every semantic
paraphrase. The benchmark must measure these rather than infer success from
validator tests.

Next: finish this PR's CI/reviews, then #4747 on a separate task branch.
No device action is requested.

## 2026-10-31: builder registration

Role first pushed as `1d20d4900` on `lamp-tiers`, then accepted on the Queue.
No lamp is assigned. Only `lamp-tiers` and `tiers/*` may be pushed; no merge or
product integration is owned by this builder.
