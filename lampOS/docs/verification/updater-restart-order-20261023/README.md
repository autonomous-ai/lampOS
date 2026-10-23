# Updater restart order, 23 October 2026

First assigned the change on Queue #3126 at 07:36 +07: derive forward and
rollback startup order from the units, with tests before code. The base is
public `f477d6c49732dbd5cba84cbe18ea568692c056e6`.

## Before the fix

`test-first.patch` adds only the integration regressions and module entries.
Production source was unchanged. Both commands ran on Darwin arm64,
Rust 1.96.0, with the same 6,529 bound files throughout each command:

```
cargo +1.96.0 test -q --locked --offline -p lamp-update --test round restart_order:: -- --nocapture --test-threads=2
cargo +1.96.0 test -q --locked --offline -p lamp-update --test round restart_quiet:: -- --nocapture --test-threads=2
```

The ordering cases failed **0 passed / 7 failed**, and the quiet cases
**0 / 2**. Every failure reached a behavioral assertion; setup and compilation
succeeded. Complete output and commands are in the two baseline `.txt` and
`.json` pairs; `baseline-manifest.json` binds every input. The test patch's
SHA-256 is `63db8a5d3093b3794b6392f7087ef9b93335c33466e5167a7b9214ad63f4b373`.

## What the tests establish

- Candidate `After=` and `Before=` override alphabetical order. Product
  Voice prerequisites come from the shipped unit, not a second runtime list.
- Rollback uses a deliberately different saved old graph, not the new graph
  or its reverse. Interrupted recovery reconstructs it without the channel,
  even with a shuffled journal and an already-restored prefix.
- Inactive and unchanged services gain no restart. Candidate and old graph
  cycles refuse before switching; an older cyclic journal remains intact.
- Speech enqueued during a prerequisite restart is drained before Voice's
  own restart. A five-second utterance postpones it until 45 seconds of
  quiet; continuous speech reaches the configured 600-second bound, puts
  already-restarted prerequisites back and never restarts Voice.

On the candidate these cases pass **7 / 0** and **2 / 0**, and the initial six
parser cases pass **6 / 0** (`positive/`). With ordering still fixed, removing
only the forward Voice quiet-boundary block makes both quiet tests fail
again, **0 / 2**. `quiet-control/` retains the exact removed block, source/test
hashes and full output; the candidate source was restored byte for byte.
This isolates the quiet protection from the baseline's Voice-first defect.

The first full crate gate passed formatting and strict Clippy, then reported
**322 passed / 2 failed / 4 ignored**. Only two older assertions still expected
Voice-first ordering for fixtures with no dependency declarations. Their
exact restart lists were changed to alphabetical tie order, with every
file, outcome, refusal and restart-count assertion retained. The complete
first output is kept in `mac-gate-1/`; it is not a waived pass. A seventh
parser test now also checks the complete shipped managed-service graph.
The final source and test delta against the base is `candidate.patch`.
Final crate and native gate receipts follow separately when complete.

## Scope and self-read

The runtime keeps the existing journal, inactive-service, file restoration
and quiet/watch contracts. It orders declared managed service restart calls;
it does not establish service readiness or resolve drop-ins, aliases,
symlinked units or external/socket nodes. Quoted names, escapes and specifiers
are refused instead of inventing or silently losing edges. The shipped Eyes
and Skills units declare themselves after Voice, and keep that relation.

A source review found no P1/P2. Its P3 on quoted tokens was corrected before
positive tests: systemd v252's dependency parser retains quoting, so this
reader refuses it rather than stripping it. See the [primary parser](https://github.com/systemd/systemd/blob/v252/src/core/load-fragment.c).
Wrong-person and stop/forget paths are not changed. The quiet boundary is
rechecked but a new utterance can still begin between the final observation
and systemd stopping Voice; there is no pause handshake. No new topic,
message, unit line or switch is introduced in this candidate. The new
ordering helper is crate-private; grep finds its callers only in Update.

V1's bootstrap announces before swapping to avoid a restart/speech race;
its software updater records active service states but uses a typed component
restart list. This change keeps the Rust updater's stronger inactive-service
rule, durable journal and bounded quiet waits, deriving order from the unit
files rather than copying that component list. No V1 or lamp benchmark ran.

These are simulated-systemd/fake-clock integration tests and Rust parser
checks on a Mac. No real service was restarted. No deployment, physical cut,
flash durability, acoustic latency or real signed release is established.

## Final Mac gate and current-base carry

The final Mac gate passes **325 / 0 / 4 ignored** (119 + 4 + 78 + 8 + 116),
with formatting and strict all-target Update Clippy clean. `mac-final/`
retains complete output and all **6,530** bound regular-file hashes. Two
ignored cases are manual fixture generation and a live GitHub download;
the other two require Linux tracing and are covered by official native CI.

The task is one commit **`ef85204dd`** on **`7bfcb31b8`**. `carry.json`
proves the runtime and tests match the final tested files. The intermediate
base changed unrelated Controls tests, installer shell helpers and Markdown;
workspace production Rust, Cargo inputs and Update's embedded unit/skill
fixtures are unchanged. The only task addition after the test is its prose
verification note. `published.patch` reconstructs the entire final commit
from its public base.

Source self-review found no P1/P2 after the parser correction and legacy
assertion updates. Nine owns the assigned external read. This candidate
changes updater restart ordering by default; First was asked whether the
new general off-switch rule includes this assigned correctness fix. No
lamp/default activation qualification is implied by the Mac gate.

Exact-commit Linux formatting also passes: `cargo fmt --all --check`,
Rust 1.96.0, in the existing cached Linux aarch64 image with network off.
`linux-format/` binds commit `ef85204dd`, source tree, image and raw output.
Fresh Mac formatting passed after the carry as well. No local ARM64
compilation/test ran; official PR jobs remain required.

## Original-head native gate

PR #3173 at `ef85204dd` passes official ARM64 run **39884293220**, job
**120188738326**, attempt 1: **7,079 passed / 0 failed / 31 skipped**,
including one leaky test. All nine new integration cases and all seven
parser cases have explicit PASS lines. Strict Linux Clippy run
**39884293240**, job **120188738516**, attempt 1 also passes.
Both checked the merge with `7bfcb31b8`; the exact checkout hashes, job
metadata, complete raw output and their hashes are in `native-ef85204dd.json`
and the two job text files. This is historical evidence for that head,
not the subsequent carry onto train 26. Nine and Reviewer-lead both posted
READ-OK with no P1/P2; the saved rollback-order recovery P3 is tracked for
a separate correction.

## Train 26 base and train 27 carry

The isolated carry on `e6b88eac2` passes the required Mac gate again:
formatting, strict all-target Update Clippy, and **325 passed / 0 failed /
4 ignored**. All **6,648** bound files stayed unchanged across each command
and the subsequent local commit `4df51b492`. `mac-train26/` retains the full
logs, file hashes and reconstructible local patch. Its runtime/test files
are byte-identical to the originally reviewed head; its docs fix the
intervening compatibility section and explain the 3600-second mixed-build
window in both directions.

Train 27 (#3176) had already frozen public `ef85204dd`, so that source
branch was not rewritten. The new prose goes with the saved-order follow-up.
`train27-carry.json` verifies the train's `71ace0101` has the same stable
patch ID as the reviewed commit and all six complete runtime/test files
match. Train head `02597ec8e` includes `e6b88eac2`. This scoped source
comparison and isolated crate gate are not a test of the complete train;
its official ARM64 and Clippy jobs provide that composition gate.
