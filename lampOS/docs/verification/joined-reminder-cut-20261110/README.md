# Interrupted joined reminders — 10 November 2026

A joined late-reminder announcement can be cut after 85% of its PCM has
been handed to the speaker while its remaining tail can contain an entire
final reminder. Previously, the aggregate mostly-heard rule completed the
first token and the consumer copied that completion to every joined token.
There is no per-item audio boundary proving that final reminder was heard.

The repair retains whether the admitted offer has other items in its
in-process receipt. Partial-cut completion refuses joined offers, which
then follow the existing bounded retry-or-held handling. Without a prior
eligible generation retirement, the successful owner fence produces
qualified retirement. A receipt with an earlier generation retirement
instead drops as `Abandon`, with its existing once-more/held handling. This can repeat
an earlier item that was heard; it does not claim that an unplayed tail was
delivered. The existing single-item rule and full-play completion stay intact.
No wire field, worker, timer, I/O, retry count or deployment switch is added.

## Regression evidence

The first probe ran on `90d9948c4af3a9b5e9912ea9e8cef77d1729bcc3` with
Rust 1.96.0, two build jobs, the shared host Cargo target and `TMPDIR=/tmp/q`.
The working branch is based on `aef04ab49`; its only intervening change is
the reader-timeout rule in `docs/joining.md`, with no runtime difference.

```sh
cargo test -q -p lamp-voice --lib a_cut_joined_reminder_does_not_complete_its_unplayed_tail -- --test-threads=1 --nocapture
```

The real coordinator receipt and durable consumer ledger are used. The
single-item control executes first and completes as intended. The joined
case then fails because the final token has durable completion despite
4,800 of 32,000 synthetic samples remaining queued. Result: **zero passed /
one failed**, 7.17s; [unedited fail-first output](fail-first.txt).

With the production guard, the same regression passes: **one passed / zero
failed**, 1.53s; [unedited repaired output](fixed.txt). Both joined items now
receive the existing qualified retirement, marked as heard in part and
eligible for the bounded repeat. This is synthetic state/ledger evidence,
not a spoken-content alignment or physical speaker measurement.

## Further checks

The real speaker-IPC/full-play positive, partial-cut regression with its
single-item control, and manual host receipt-decision latency diagnostic
passed together: three tests, zero failures, 5.03s. See [the raw controls
and timing output](controls.txt). Command: `cargo test -q -p lamp-voice
--lib joined_reminder -- --include-ignored --test-threads=1 --nocapture`.
Strict lint and broader reminder results are recorded below. The latency target for the added receipt predicate is
under 1 ms, with no additional wait on the input-to-cut path. The diagnostic
excludes receipt allocation, IPC, playback, cancellation and ledger writes;
it cannot establish acoustic or device response time.

No device, deployment or activation was performed. Two current-head reads
and the product's normal lamp qualification remain separate from host tests.

Host debug receipt-decision measurements, 256 fresh receipts for each shape:

| Offer | First call | p95 | Maximum |
|---|---:|---:|---:|
| Single item | 7.165 µs | 0.201 µs | 7.165 µs |
| Joined items | 0.396 µs | 0.055 µs | 0.396 µs |

The measured decision boundary meets the stated 1 ms target on this host.
These compare two offer shapes after the fix, not before/after versions.
The manual test is ignored by default because host timing is not a portable
CI assertion; it asserts the functional decision for every timed receipt.

Documentation validation passed after installing the lockfile's cached
packages with `npm ci --offline --ignore-scripts --no-audit --no-fund`: nine
checker tests, 192 documents, 1,898 local links, zero broken. An initial run
before installing dependencies failed with missing `markdown-it`; no source
change or lint suppression was used to resolve that setup error.

An internal source spot-check found no alternate completion caller bypassing
this guard. It identified the distinct earlier-generation retirement case,
so the documentation distinguishes qualified retirement from abandonment.
The partial-cut fixture models a successful owner fence explicitly; it does
not reproduce a failed cancellation or a physical audio interruption.

## Final source checks

The final partial-cut regression also includes a prior PCM-generation
retirement. It checks that the joined tail remains uncompleted and the
receipt follows `Abandon`, while the no-prior-retirement case uses qualified
`Retire { heard_some: true }`. Full playback still completes both items.

Commands from `lampOS/`, with the same host environment as above:

```sh
cargo fmt --all --check
cargo clippy -q -p lamp-voice --all-targets -- -D warnings
cargo test -q -p lamp-voice reminder -- --test-threads=4
cargo test -q -p lamp-voice --lib reminders::transport::tests::lost_wait_ownership_drop_retains_claim_until_fixture_exit -- --exact --test-threads=1 --nocapture
cargo test -q -p lamp-voice reminder -- --test-threads=1
```

- Strict all-target lint passed; [its empty successful output](clippy.txt).
- Final parallel reminder run: **176 passed / one failed / three ignored**,
  48.50s; [raw output](parallel.txt). The unchanged transport subprocess
  fixture timed out during its bounded handshake. The earlier run before
  adding the prior-retirement variant passed 177 / three ignored in 40.46s.
- The exact failed identity passed once in isolation, 3.40s;
  [raw diagnostic](isolated.txt). No transport source or timeout changed.
- Final serial reminder run: **177 passed / zero failed / three ignored**,
  50.83s; [raw output](serial.txt). The other selected test targets had zero
  matching tests and completed successfully. Both commands exited zero.

The serial pass does not erase the parallel failure or prove its cause.
The three ignored tests are the new manual receipt timing diagnostic
(explicitly run above) and the existing `reminders::tests::parent_poll_cost`
and `reminders::transport::tests::poll_cost` host-cost diagnostics. Normal
CI does not assert these host timings.
Linux qualification remains separate from these native Mac results.
