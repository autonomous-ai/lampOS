# Forget boundary and interrupted send receipts — 10 November 2026

Third's repair is based on Fourth's current context carry, #5913 at
`b0e70d95404e31d86307471e14abc1f8df553763`, on `a2260fd1b85ed901ced9d1a083c893874b1d6bd3`.
The published negative proofs are preserved from `6c82603f93`, `e5c38fcb26`,
`fb77f3f718` and `920524f549`. The last is the real services-fallback proof.
No device access, deployment or live-account writes were performed.

## Failure and repair

An interrupted send can outlive its original turn. Recovering its receipt
following an admitted forget could recreate the discarded tool call and
result, including through a late watcher or services receipt fallback.
Removing accounting would allow duplicate sends or falsely deny a send.

The repair retains request, attempt, timing and send-outcome accounting,
while sharing a content epoch across retained and copied handles. Admission
advances that epoch, clears carried rounds and invalidates receipt publication.
The latency follow-up below removes the original eager receipt sweep; raw
bounded receipt payloads are scrubbed at guarded consumption/completion or
dropped at eviction. No immediate RAM-zeroization claim is made. Recovery and final
publication serialize with admission; no lock spans an async receipt wait.
Copied carried rounds and late ordinary batches keep their producer epoch.
A stale batch cannot acquire a fresh display epoch merely because its send
starts later. New context after the local forget reset gets a fresh epoch.

Confirmed sent, unknown/pending, and refused outcomes retain distinct fixed
notes. Post-forget pending outcomes do not claim success. Existing repeat
protection remains. The repair changes neither deletion authorization nor
provider/audio cancellation.

## Local evidence so far

All Cargo commands use the pinned Rust 1.96 toolchain from `lampOS/`, with
`CARGO_BUILD_JOBS=2`, `TMPDIR=/tmp/q`, and the shared target directory
`/Users/d/code/autonomous-os/lampOS/target`.

Before the production repair:

```sh
cargo test -q -p lamp-voice --lib second_provider::tests::sent_write:: -- --test-threads=1
```

Result: **9 passed, 3 failed**, exit 101. The failures are actual Engine
canary-restoration assertions for send/cut/forget/cut, late completion and
services fallback; duplicate-dispatch controls passed.
See [the unedited fail-first log](fail-first.txt).

Intermediate repaired source passed **18 checks**, including the manual
latency diagnostic, using:

```sh
cargo test -q -p lamp-voice --lib second_provider::tests::sent_write:: -- --test-threads=1 --include-ignored --nocapture
```

See [the unedited intermediate log](intermediate-focused.txt). This run
predates the final producer-epoch propagation into late-started sends; it is
not final-head qualification. Final gates and independent reads remain pending.

## Host timing boundary and limitation

The diagnostic times only `forget_carried_context` entry through return on
an already constructed host Engine, in the debug test build. Setup/allocation
and correctness assertions are outside the timer. No locks are contended.
It measures first use, 256 repeated empty clears, and 64 fresh saturated
ledgers. Saturation is 8 requests × 64 calls = 512 receipts, each with 1 KiB
argument text and 4 KiB result text, plus JSON and IDs: 2,621,440 variable
content bytes. No carried rounds are present. This is representative bounded
payload, not maximum wire-response size.

| Case | Median | p95 | Maximum |
| --- | ---: | ---: | ---: |
| First empty call, n=1 | 1.114 µs | 1.114 µs | 1.114 µs |
| Repeated empty, n=256 | 0.156 µs | 0.176 µs | 0.684 µs |
| Saturated receipts, n=64 | 3.342 ms | 4.102 ms | 4.325 ms |

The saturated debug case **does not meet the initial <1 ms host target**.
The boundary performs no network, disk, polling or async wait, but synchronous
redaction/deallocation scales with retained content. No release-build,
contended-lock, physical-lamp, acoustic or end-to-end latency claim follows
from this diagnostic. Deletion and slow provider work are outside its boundary.

## Broader local gate

Formatting passed with `cargo fmt --all --check`. Strict all-target Clippy
passed in 4m 59s on the final production source with:

```sh
cargo clippy -p lamp-body -p lamp-cloud -p lamp-control -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-name -p lamp-peek -p lamp-setup -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings
```

The docs checker passed its 9 tests after `npm ci --ignore-scripts --offline`;
`npm run check` reported 192 docs, 1,899 local links, zero broken. The first
attempt lacked the worktree's npm dependencies and failed before checking;
the offline install resolved that environment prerequisite.

The full `cargo test -p lamp-voice` gate ran outside the sandbox so local
Unix socket fixtures could bind. Its result and the remaining-target checks
are recorded below. Linux/ARM64 CI and external independent reads remain pending.

The broad Voice library result is **5,475 passed, 5 failed, 18 ignored**,
469.72s, exit 101. See [the full unedited log](voice-full.txt). Four motor
child-process tests timed out, and the reminder lost-wait ownership fixture
missed its bounded child handshake. All five passed individually using the
same compiled test executable, unchanged timeouts, short `TMPDIR` and
`--exact <name> --test-threads=1 --nocapture`; exact names/commands and exits
are in [the isolated follow-ups](isolated-failures.txt). This is not a
retrospective full-suite pass. No unrelated motor/reminder source was changed.

The same binary's final focused run, with
`second_provider::tests::sent_write:: --test-threads=1 --include-ignored --nocapture`,
passed **19 checks**, zero failed, 3.11s; see
[the unedited final focused log](final-focused.txt). It includes all 18
functional regressions/controls and the manually enabled timing diagnostic.
The only later Rust edit before workspace build was the explanatory comment
on the after-return `drop_carried_rounds` helper, not executable code.

Final timing sample: first empty 2.455 µs; repeated empty median 0.142 µs,
p95 0.159 µs, max 0.556 µs; saturated ledger median 1.730 ms, p95 1.881 ms,
max 1.972 ms. Both captured saturated samples miss the <1 ms target. The
lower second sample is not claimed as a latency improvement: code affecting
that diagnostic was unchanged and host load varied.

`cargo build --workspace` passed in 5m 03s. `cargo test -p lamp-voice --doc`
passed (zero doctests defined). Their unedited output is retained in
[workspace build](workspace-build.txt) and [documentation tests](doctest.txt).
The remaining binary/integration targets are run explicitly with:

```sh
cargo test -p lamp-voice --bins --test '*'
```

Binary/integration gate passed: **280 passed, zero failed, six ignored**
(1 binary test, 277 integration tests, 2 probe-absence checks). The six
release-pair cases require pinned executables on isolated native Linux ARM64;
this Mac run does not satisfy that requirement. See
[the unedited integration output](integration.txt). Docs links and source/document whitespace checks passed again. Unedited
command output retains its original trailing spaces and blank final lines;
`git diff --check` reports those log-only whitespace warnings. The local record retains all five original
broad-run failures and their isolated passes; native CI and independent
review remain required before the repaired stack is ready.

## Admission-latency follow-up

Second independently confirmed the original 512-entry sweep missed the
<1 ms host target (p95 2.317 ms, max 4.932 ms). The follow-up removes the
receipt/content/watch sweep from admission. The shared epoch invalidates
publication immediately; registration, completion and recovery already check
that epoch before exposing a display payload. Carried rounds still clear at
admission. There is no new task, I/O or receipt wait on that path. Raw bounded
payloads may remain until those guarded accesses or eviction; this is a
publication boundary, not immediate erasure of every memory copy.

A new busy-content-lock regression failed on the prior implementation:
**0 passed / 1 failed**, 1.01s, because admission waited for the receipt lock.
The fixture releases the lock and joins even on failure, so it cannot hang.
See [the fail-first output](busy-fail-first.txt).

With the follow-up, the full focused command above passed **20 checks**,
zero failures, 3.32s: 19 functional checks and the manual diagnostic. The
saturated diagnostic now consumes all eight origins after timing admission,
verifying every old call/result stays out of carried rounds, each confirmed
send retains its duplicate guard, and all 512 entries are scrubbed by actual
recovery. See [the unedited follow-up output](lazy-focused.txt).

Same host debug boundary, inputs and sample counts as the prior diagnostic:

| Case | Prior median / p95 / max | Follow-up median / p95 / max |
| --- | --- | --- |
| First empty call, n=1 | 2.455 / 2.455 / 2.455 µs | 0.421 / 0.421 / 0.421 µs |
| Repeated empty, n=256 | 0.142 / 0.159 / 0.556 µs | 0.096 / 0.113 / 0.413 µs |
| Saturated receipts, n=64 | 1.730 / 1.881 / 1.972 ms | 1.246 / 2.073 / 2.720 µs |

The follow-up meets the original <1 ms target for this uncontended host
measurement. It moves receipt scrubbing to guarded consumption; it does not
claim the cleanup work disappeared or all paths became faster. The timer is
still function entry through return and excludes payload setup, subsequent
recovery, carried tool rounds and lock contention. Recovery can itself hold
the publication lock while acquiring a content lock, so admission can still
wait indirectly behind concurrent recovery. The busy-lock test covers the
direct dependency only. No release/device/acoustic latency claim follows.

Follow-up broad gates and independent rereads are pending; earlier native and
review results remain bound to the pre-follow-up implementation.

### Completed-evicted control and integration base

Fourth requested a completed, evicted receipt in addition to the pending
completion case. The added test polls actual `sent_with_words` recovery
through a completed raw receipt to a second pending try, then evicts the
ledger and admits forget. Recovery later keeps only the fixed sent note and
repeat guard; the copied completed watch is scrubbed and no old tool round
is published. The focused gate now passes **21 checks**, zero failures,
3.33s. Its repeated saturated diagnostic remains below target: p95 2.362 µs,
max 2.999 µs. See [the exact follow-up log](lazy-final-focused.txt).

The follow-up broad library run before that additional test was compiled
reported **5,471 passed / 10 failed / 18 ignored**, 479.56s. Nine motor
child-process/step-watchdog timeouts and one reminder child handshake timeout
all passed individually with the same compiled binary and unchanged limits.
See [the broad run](lazy-voice-full.txt) and
[its isolated follow-ups](lazy-isolated.txt). No unrelated source or timeout
was changed, and the broad run is not relabelled as passing.

The original eight-commit stack was then rebased onto runtime integration
`a9eb18ecea88714510e66b4248879e963a40130a`, with every range-diff entry equal;
its new baseline is `3e2b4fd770e65b3991286b74bee3a79e0b6e5f13`.
At the rebase checkpoint, the uncommitted latency follow-up's stable patch ID remained
`2eb9bb64e8f92d5295ccf3106fdb1f1ad340da06` before and after the rebase.

That baseline's complete tree is `a941b4347b6d1328f1845cbf55be4552ea0a378d`,
exactly the actual merge tested by prior-head ARM run
[42734894351](https://github.com/autonomous-ai/lampOS/actions/runs/42734894351):
checkout `64d18ebac54b88ad2522b13e9db59b1fee7d5e0d`, parents a9eb + 0036acfaa.
It passed **14,959 tests / 112 skipped** in 305.038s, plus the other workflow
steps. This qualifies the baseline composition, not the uncommitted latency
follow-up. Fresh local composition gates and final-head native CI are kept
separate. The watcher briefly lost its GitHub connection; the job continued
and ultimately succeeded.

### Final local composition on a9eb

`cargo fmt --all --check` and the same strict 14-package all-target Clippy
command passed on the rebased baseline plus this follow-up (Clippy 2m 35s).
See [the exact composition Clippy output](composition-clippy.txt).
The focused command passed **21 checks / zero failures**, 3.34s. Saturated
host timing was median 0.716 µs, p95 1.336 µs, maximum 2.200 µs under the
same stated boundary and exclusions; see [its output](composition-focused.txt).
The documentation checker again passed 9 tests and found 192 docs, 1,899
local links and zero broken links. Broad composition gates are recorded below.

The final composition library run reported **5,482 passed / 9 failed /
18 ignored**, 481.17s, exit 101. Eight motor process startup/step/watchdog
cases and one reminder child handshake failed. All nine passed individually
with the same binary and unchanged timeouts. The extraction also selected the
nested reminder fixture's printed failure name: its separate invocation passed
immediately without the parent fixture environment and adds no evidence beyond
the successful parent rerun. Thus the ten commands are nine meaningful isolated
reruns plus that fixture-only invocation. See [the full run](composition-voice-full.txt)
and [all isolated commands and exits](composition-isolated.txt). The broad
run remains failed; no source or timeout was changed to obtain those passes.

Remaining composition targets: `cargo test -p lamp-voice --bins --test '*'`
passed **280 tests / zero failures / six ignored**. The six still require
isolated native ARM64 release executables. `cargo test -p lamp-voice --doc`
passed with zero doctests defined. See [integration output](composition-integration.txt)
and [documentation-test output](composition-doctest.txt).

`cargo build --workspace` also passed on this composition; see
[the unedited workspace build](composition-build.txt). Final-head Linux/ARM64
CI and Second/Fourth's rereads of the latency follow-up remain required.

Before publication the stack was rebased onto `4d86f6f0d`, which adds only
two `docs/joining.md` policy edits above a9eb. All eight baseline commits
remain range-diff equal, and both changed Rust files were byte-compared
against the pre-rebase autostash and are unchanged. The completed runtime
gates therefore cover the published source; no new runtime composition is
implied by this documentation-only rebase. Workspace build took 1m 45s.
