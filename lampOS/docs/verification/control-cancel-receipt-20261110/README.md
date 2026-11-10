# Pending turn cancellation receipts

Fourth claimed the stalled #5870 carry in comment6755735299 after checking
open and closed replacements. Original author commit
c7f1ef42eff5f2d3a7b070a9977f7a12cdf72219 is preserved as
38f98f3f4e0560ee1a7046c1cb2665b43007a679 on base
a2260fd1b85ed901ced9d1a083c893874b1d6bd3; range-diff reports equality.
The carry was subsequently rebased to a9eb18ecea88714510e66b4248879e963a40130a
as 9847aa423d16aa754fc362e8b7b92dc9cd8d8788. Its only range-diff
change resolves the SDK guide against upstream measured build status while
preserving the carried API semantics. The reproductions below remain evidence
from the earlier a226 base.
The later documentation-only integration update 4d86f6f0ddaf5a8c645fbf5ed901605444ea2cef
rebases the carry unchanged to 22d876382c2287e253a7dff429281b5233bd61b8.
The saved repair diff remained byte-identical after autostash restoration.
The original branch remains untouched. Its prior lamp result does not qualify
this follow-up. No device action was performed.

## Reproduction on the carried implementation

Two actual Lamp/Reply controls with synthetic Unix voice peers failed:
zero passed, two failed, zero ignored, 2.70 s body (185.694 s command).
One peer accepted cancellation and then closed without an outcome; turn.end
incorrectly returned ended. Two pending starts acknowledged after 1.3 s and
another 1.0 s; the successful reply arrived at 2.317005081 s, beyond one
shared two-second budget. The measurement ends when the reply returns,
before joining the deliberately late peer.

A separate real-socket backpressure control failed: zero passed, one failed,
zero ignored, 0.12 s body (24.192 s command). Filling the fixture's send
buffer made reply dispatch take 102.155995 ms, exceeding the 50 ms host
input-handling target. Cleanup occurred before the failing assertion.

Both runs used Rust 1.96.0, short owned TMPDIR, the single retained Cargo
target, DEBUG0, no incremental compilation and two build jobs. Sources
remained frozen during each run. Logs/receipts are retained outside the
repository as /tmp/fourth-control-cancel-red-* and
/tmp/fourth-control-cancel-write-red-*.

## Repair and latency boundaries

The path is control reply dispatch → cancellation on each pending start's
own Unix connection → explicit voice outcome → caller reply. Targets are
less than 50 ms for dispatch with a slow peer, and one two-second budget
for the cancellation writes and all receipts. Scheduling delay on a paused
host and acoustic/device response are not measured by those targets.

An internal receipt distinguishes an explicit say outcome from transport or
protocol uncertainty. Existing done events still report dropped on transport
failure, while pending turn.end refuses to confirm completion. Missing,
malformed or wrong-operation outcomes cannot count as acknowledgement.
Explicit spoken remains already-finished refusal; dropped, expired and
refused retain their existing meanings.

Pending starts are marked under the voice-state lock, retaining the original
connection for client loss/revocation. The existing deferred-reply worker
owns copies for cancellation writes, outside the door and voice locks; no
additional worker is introduced. One deadline is acquired before that state
lock and reused for every write and receipt. Partial writes and interrupts
cannot renew it. A failed/blocked/expired write half-closes the request side,
which the voice's existing EOF watcher uses to drop a still-queued start,
leaving its response side readable. EOF does not cancel an already-begun
prompt or turn: a failed write cannot claim that cancellation was delivered. Not-yet-connected starts retain their cancellation
and send it when they connect. A refused reply does not retract the cancel request and never guarantees
that a failed write reached the voice.

Permanent controls cover the two handshake regressions, first/repeated
backpressured dispatch, explicit/missing/wrong-operation receipts from two
starts, and request-only half-close preserving the response. Existing
per-client ownership, prompting/open cancellation-before-admission and
already-finished controls remain. A real voice-socket control also checks
that request half-close drops a queued start with a readable receipt, while
an already-begun start remains uncancelled and returns its actual outcome.

## Validation

The repair is not ready. The following results bind the frozen repair on
22d876382 / base 4d86, not the later integration runtime. Rust 1.96.0,
`--offline`, DEBUG0, no incremental compilation, two build jobs, short
TMPDIR and one shared target were used throughout. All tested production
hashes stayed unchanged; exact command arrays and logs are retained in
`/tmp/fourth-control-cancel-4d86-{receipt,remaining-receipt,tail-receipt}.json`.

- `cargo fmt --all --check` passed (23.268 s).
- `cargo clippy --offline -q --workspace --all-targets -- -D warnings`
  passed (163.742 s).
- Five focused pending-cancel tests passed. Backpressured dispatch measured
  27.255 / 24.712 / 22.464 us against the earlier 102.156 ms failure;
  two missing receipts replied in 2.009443513 s against 2.317005081 s.
  These are host socket boundaries, not acoustic or device latency.
- The actual Voice half-close test passed; full Control passed 196 tests.
- Full Voice failed: 5,467 passed, four failed, 16 ignored, 1,468.61 s body.
  Privacy lease resync exhausted its 20 freshness attempts; declined-report
  consent missed its tool response after a logged 658 ms hold; two idle
  connection tests counted four versus three and three versus two. Each
  passed one exact diagnostic on the same source. No full-suite retry was
  performed, and those diagnostic passes do not erase the full failures.
- Main Voice integration failed: 276 passed, one failed, 38.22 s body.
  `twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
  completed all 20 calls, but the middle latency was 63.128396 ms against
  25 ms (range 3.302230–127.996926 ms). No latency retry or relaxed bound.
  Remaining integration targets passed two cases / six ignored, binaries
  passed eleven, and documentation tests passed with zero cases.
- Cloud, Memory and Voice Eval passed. Voice Eval passed 625 cases total.
  Memory's command took 1,368.788 s; Voice Eval took 287.405 s.
- Update failed (1,368.168 s command). Library 421 passed / two ignored,
  two six-case targets passed and its 160-case integration target passed.
  The separate `it` target passed 13 / failed four journal-retention cases:
  `a_boot_the_owner_forgot_is_not_kept_at_its_shutdown`,
  `a_failed_move_keeps_the_kept_boots_and_says_so`,
  `the_kept_boots_are_kept_72_hours_at_most`, and
  `the_last_two_boots_are_kept_at_shutdown_only`. Two report missing files;
  failed-move explicitly reports BSD `stat` rejecting GNU `-c` and arithmetic
  failure; retention found zero directories instead of two. No broader
  root cause is inferred, and no retry has been performed.

Package suites used `cargo test --offline -q -p <package> --
--test-threads=2`; Voice targets were split after recording the full
failure. The remaining dependent packages (Skills, Body, Expression,
Keeper, Lean, Name, Peek, Setup) and workspace build were pending at that
point; their completed results follow below. After
preserving these outcomes, current-runtime composition gates, current-head
Linux CI and Sixth/Second reads remain required. No device qualification.

The earlier a226 repair run passed formatting (26.321 s), then was
interrupted during Clippy when integration gained runtime changes. That
Clippy did not complete; no repaired tests or build ran in that epoch.

The resolved SDK guide passes all seven existing checks (3.24 s), run from
`lampOS/sdk/python` with:

```sh
UV_CACHE_DIR=/tmp/fourth-sdk-test-cache PYTHONPATH=src uv run --no-project --no-build --python /usr/local/bin/python3.12 --with 'pytest>=8' --with 'websockets>=13' --with 'cryptography>=42,<47' python -m pytest -q -p no:cacheprovider tests/test_guide.py
```

System Python lacked pytest, so that attempt ran no tests. An initial
isolated installer unexpectedly began a cryptography source build; its
identified process group was stopped (exit 143) and verified gone. The
successful retry required binary wheels only. No repository dependency was
changed. Documentation links also pass: 192 documents, 1,898 local links,
zero broken. These checks do not replace the pending Rust repair gates.

The SDK GUIDE now also records the shared two-second cancellation budget and
explicit-receipt requirement, including uncertain transport/protocol refusals.
This documentation update does not change the frozen Rust source or the
recorded host/device qualification boundaries.

Guide-only verification on 10 November: the default `python3 -m pytest` could
not start because pytest was not installed in that interpreter. Reused the
existing pytest environment read-only with bytecode and pytest cache disabled:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=lampOS/sdk/python/src /tmp/r3-5801-venv/bin/python -m pytest -q -p no:cacheprovider lampOS/sdk/python/tests/test_guide.py
```

All seven checks passed in 0.16 s; documentation links passed 192/1,898/zero
broken. This does not change or rerun the Rust qualification above.

## Remaining old-base dependency gates completed

The previously unrun stages on 22d876382c2287e253a7dff429281b5233bd61b8
plus this repair completed successfully, with all three frozen Rust hashes
unchanged. Each package used `cargo +1.96.0 test --offline -q -p <package>
-- --test-threads=2`; workspace build used `cargo +1.96.0 build --offline -q
--workspace`. Environment and profile remain as recorded above.

| Package | Passed | Ignored | Command seconds |
| --- | ---: | ---: | ---: |
| Skills | 664 | 9 | 343.497 |
| Body | 740 | 1 | 104.841 |
| Expression | 361 | 0 | 70.184 |
| Keeper | 42 | 1 | 41.663 |
| Lean | 61 | 2 | 5.253 |
| Name | 57 | 3 | 120.224 |
| Peek | 26 | 0 | 27.646 |
| Setup | 287 | 1 | 179.036 |

Workspace build passed in 183.036 s. Controller exit zero; receipts and
stage logs are `/tmp/fourth-control-cancel-4d86-final-*`. None of these
successes replaces the earlier full Voice, light-call latency or Update
failures; those stages were not retried. The old-base dependency run is now
complete. Current-runtime source composition, new native/ARM/Linux gates and
fresh reads remain pending; this is not device acceptance.

## Pinned current-runtime carry

After the old-base stages ended, the authored carry rebased to
`a32cad0be477e56cc058cdfc18ac0344c232feb8` on pinned integration
`c29008d21076053edf0a990f4b9e2cf5e106f3f2`. Its authored commit is
range-diff equal; every tracked repair addition/deletion and the untracked
verification note survived autostash unchanged. Incoming Control microphone
fields and binary callback changes do not replace the pending-turn ownership,
cancellation or receipt path. The preserved repair is checked against the
new Control/Voice runtime, not qualified by the old-base results alone.

Current gates are workspace formatting/strict Clippy, full Control, Voice
say/control-turn/ring-worker/language/sent-write/held-forget/held-rename
filters, full Voice, full Setup and workspace build. Current-source
ARM/Linux CI and Sixth/Second reads follow publication. Disk availability
is checked between stages; a resource stop is not a test failure or pass.
The completed native results and remaining gates are recorded below. No device or activation claim.


## Current-base native qualification, 10 November 2026

The six guarded source/document hashes stayed unchanged throughout the main
controller, remaining checks and latency capture on `a32cad0be4` plus this
repair, based on `c29008d210`. The verification note was extended only after
all three controllers were terminal. Rust 1.96.0, the same short TMPDIR and
single shared target described above; two build jobs, debug info and
incremental compilation disabled.

- Workspace formatting passed in 24.091 s; strict workspace/all-target
  Clippy passed in 247.657 s.
- Full Control passed 218 cases, zero failures/ignored, 154.029 s command.
- Focused Voice groups passed: say 99, control_turn 14, ring_worker 14,
  language 25, sent_write 20 /one ignored, held_forget 31, held_rename 18.
- Full Voice library **failed**: 5,505 passed /one failed /18 ignored,
  1,264.92 s body and 1,341.389 s command. The failure was
  `ring_night::tests::the_worker_rests_on_the_asleep_face_only_while_expression_sleeps`,
  `ring_night.rs:499`, assertion `shows(Some(true), false)`. That source file
  is identical to the pinned base. Its cause remains unestablished.
- One restricted diagnostic could not bind a Unix socket at line 440,
  before the original assertion. The restricted integration attempt had
  209 passes /68 failures: 66 failure sections explicitly contain a
  permission error; the other two are `news_told` five-second timeouts.
  Those two causes are not inferred from the permission failures.
- With local socket permissions enabled and source unchanged, the exact
  ring-night diagnostic passed once (2.69 s body, 5.435 s command).
  This does not erase or diagnose the original full-library failure.
- Previously unrun Voice integration passed 279 /six ignored, 28.491 s
  command; binaries/examples passed eleven, 14.522 s; doctests had zero
  cases, 4.757 s. No full-library rerun was used to obtain these results.
- Full Setup passed 292 /one ignored, 128.961 s; workspace build passed
  in 151.822 s. The remaining controller exited zero.

Main commands were `cargo +1.96.0 fmt --all --check`,
`cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings`,
`cargo +1.96.0 test --offline -q -p lamp-control -- --test-threads=2`,
`cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2`,
`cargo +1.96.0 test --offline -q -p lamp-setup -- --test-threads=2`, and
`cargo +1.96.0 build --offline -q --workspace`. Focused Voice commands use
`--lib <filter> -- --test-threads=2`; remaining Voice commands explicitly
select `--test it --test memory_probe_absent --test release_pair`, then
`--bins --examples`, then `--doc`. The diagnostic uses the full exact test
name with `--exact --nocapture --test-threads=1`.

Raw logs, exact commands, elapsed times and hashes are retained under
`/tmp/fourth-control-cancel-current-*`, including separate
`remaining-*` (restricted) and `unsandboxed-remaining-*` receipts.
The failure report is Queue comment 6760563543. Earlier old-base failures
remain above. Current ARM/Linux CI and fresh source reads remain required;
this note makes no all-green native or ready-to-merge claim.

### Current-base host latency

After the remaining build completed, each of the two latency cases ran once
with `cargo +1.96.0 test --offline -q -p lamp-control --lib
parts::tests::<case> -- --exact --nocapture --test-threads=1`. Both passed.

`a_pending_cancel_write_is_off_the_door_lock` measured the return from
`Lamp::reply` under real Unix-socket write backpressure: first dispatch
140.807 us, then 72.723 and 57.554 us, against the original failed
102.155995 ms sample and the under-50 ms host target. This excludes the
later asynchronous cancellation outcome and device feedback.

`pending_cancels_share_one_reply_deadline` measured dispatch through the
returned deferred reply for two pending starts: 2.005037210 s, versus the
original 2.317005081 s. Both cancellation writes and receipt waits share the
same two-second budget; scheduler/return overhead is included in elapsed
measurement. It does not establish a two-second hard wall-clock guarantee.
Logs/receipt: `/tmp/fourth-control-cancel-current-latency-*`.

Current SDK guide tests passed seven in 0.16 s using the existing Python
environment with bytecode/cache writes disabled. No model, acoustic,
physical output, device deployment or consent/default acceptance was run.
