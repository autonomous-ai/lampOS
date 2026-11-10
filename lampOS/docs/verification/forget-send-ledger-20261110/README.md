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
advances that epoch and clears retained display payloads. Recovery and final
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
