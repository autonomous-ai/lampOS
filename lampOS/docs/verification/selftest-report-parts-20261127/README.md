# Self-test report names and serialized size: host verification

Part of #8793. Current self-test checks name the five joints and four other
checks that the cloud report's fixed list omitted. For example, an elbow
failure was counted only in `other_failed`. The report now retains its fixed
`elbow` name. This changes the vocabulary inside the existing `lamp-report/1`
shape, without changing a source verdict or inventing a new test run.

The allowlist keeps all 16 previously supported names and adds nine names from
the current producer. A source census follows the actual registered checks,
their `Check::part` implementations and the joint-name constant. Seventeen
current names plus eight legacy-only names give 25 supported values. A new
registration or naming form requires a source and privacy review.

## Original candidate checks

From `lampOS/`, using Rust 1.96.0 on x86-64 macOS:

```sh
cargo fmt -p lamp-cloud --check
cargo test --offline --locked -q -p lamp-cloud --lib reports::
cargo clippy --offline --locked -q -p lamp-cloud --all-targets -- -D warnings
TMPDIR=/tmp cargo test --offline --locked -q -p lamp-cloud
```

Before the serialized-ID correction below, the focused report suite passed
**70 tests**, with one existing ignored test.
Strict all-target Clippy and formatting passed. The complete crate gate passed
**590 tests**: 542 library, one binary and 47 integration tests; four existing
tests were ignored across the library and documentation suites. The full
crate gate and strict Clippy were rerun successfully at
`c7982f131fd94bda06af8caba9478c9be7907e3f` after a dependency rebase. These are
old-head results, not evidence for the subsequent size correction. Builds
used two compiler jobs and an existing dependency cache.

The initial focused run could not bind local fixture sockets inside the
restricted sandbox. A rerun with local socket access passed. The first full
crate run then reached three macOS Unix-socket path-length failures at
106–107 bytes under the host temporary directory. Invocation-scoped
`TMPDIR=/tmp` resolved that cause without a source change. The failed logs were
retained; neither failed attempt is counted as a pass.

The two new tests prove current producer names and legacy names survive the
projection, unknown failures remain counted, and canary part names, reasons
and reading strings are absent. A report containing every supported failure
and every diagnostic part fit the existing 4 KiB limit with a 128-byte
unescaped ID. That check did not cover the additional JSON bytes for a
permitted escaped ID. Repeated producer entries that exceeded the admission
limit were rejected and counted as too large.

The original production change added nine fixed strings to the existing
lookup, with no input callback, I/O, thread, scheduling or network operation.

## Serialized-ID correction

The review of #8917 found that a 128-byte graphic ASCII ID can need 256 bytes
inside JSON. The old admission reserved only 128. The public API reproduction
uses 282 legacy `voice_mic` failures, so the gap is independent of the nine
new names. The new admission regression failed against the old code because
that report was queued; a second regression failed because an older saved
report above 4 KiB after ID insertion was handed to the connection.

Admission now reserves the worst escaped ID plus its JSON key. The existing
delivery serialization also checks its final byte count before setting
`handed`; it is not serialized a second time. An oversized sendable row is
removed using the existing queue-drop policy and counted once, with one state
save for the batch. It is not acknowledged, so skill days and report counters
remain pending. Unknown fields are preserved in fitting current-schema rows;
newer schemas and kinds remain untouched. The once-only count across restart
requires successful existing file removal and state persistence: it is not a
filesystem transaction or a crash-durability guarantee. No retry loop,
replacement text, report format, consent rule or allowlist name was added.

The corrected fixtures cover quotes, backslashes, mixed IDs, every permitted
graphic ASCII byte, exact 4095/4096/4097-byte delivery boundaries, replayed
identity and source fields, old saved reports, consent, newer queues and
unconsumed counters. The skill-trimming fixture now uses the worst escaped
ID and still checks that listed runs plus `store_rest` conserve the total.
The new delivery-drop counter also saturates a seeded `u32::MAX` value
instead of wrapping it; other counter paths are unchanged.

The host timing target, set before measurement, is no more than 20% median
regression against the old delivery implementation for fitting one-health and
64-report near-limit queues. The measured boundary is `due()` send preparation:
consent read, JSON parse, ID insertion, serialization and handed bookkeeping.
Fixture capture/reset is outside the timer. The old implementation is retained
only inside the ignored measurement test for comparison. This uses the normal
debug test build and includes first-call, median, p95 and maximum times over
128 repeated calls per case and implementation. It measures neither MQTT nor
acoustic/hardware latency and makes no slow-filesystem deadline claim. Fitting
reports add no writes; the exceptional old-queue drop uses the existing file
removal and state-save path.

The explicit host measurement passed on x86-64 macOS with Rust 1.96.0. Times
are microseconds; the full queue is a synthetic 64-row input whose reports
each reach 4096 bytes after insertion of the worst escaped ID.

| Input | Implementation | First | Median | p95 | Maximum |
|---|---|---:|---:|---:|---:|
| One health | Before | 201.38 | 118.76 | 121.46 | 128.76 |
| One health | Guard | 138.10 | 118.41 | 123.95 | 161.16 |
| 64 near-limit reports | Before | 30315.79 | 30511.84 | 40599.95 | 62375.26 |
| 64 near-limit reports | Guard | 30475.00 | 30699.88 | 38042.04 | 52278.86 |

Median changes were -0.3% and +0.6%, respectively, within the declared host
target. First-call times are separate from the 128-repeat distribution and
its maximum. This is measured comparison evidence, not a timing assertion or
a target-device guarantee. Run the measurement explicitly with:

```sh
cargo test --offline --locked -q -p lamp-cloud --lib measure_due_size_guard -- --ignored --nocapture
```

### Corrected local gate

The correction applied atop `c7982f131fd94bda06af8caba9478c9be7907e3f` passed
the same Rust 1.96.0 Mac gate, with two jobs, the existing cache,
`TMPDIR=/tmp` and local fixture socket access:

- Formatting and strict all-target cloud Clippy: passed, no diagnostics.
- Focused reports: **74 passed**, two ignored (one existing fixture writer
  and the explicit timing check).
- Complete cloud package: **594 passed** (546 library, one binary, 47
  integration); five ignored (four existing and the explicit timing check).
- Explicit timing check: **one passed**; it has no flaky timing assertion.

The final `reports.rs` SHA256 for these corrected gates is
`37c19211781a18197d5ba439dab9e8f70523aac794a50ad80eb1909b9060f3fd`.
The two deliberate old-code failures remain separate regression evidence;
they are not counted as successful gates. No workspace-wide, native ARM64,
Linux or device result is claimed for this correction here.

## Remaining gates

Native ARM64 and full-workspace Linux Clippy CI remain required at the submitted
head. This default-emitted additive vocabulary needs the first builder's read
and the repository's two independent privacy reads. Closed-list downstream
readers must accept the added names before rollout. The local dashboard readers
have separate synthetic compatibility checks; private backend readers were not
inspected here.

Earlier reports cannot recover names already collapsed into `other_failed`.
Repeated summaries are still not distinct runs, failure occurrence times or
experience-success denominators. No lamp, active self-test, report setting,
consent choice, backend write or deployment was changed by this verification.
