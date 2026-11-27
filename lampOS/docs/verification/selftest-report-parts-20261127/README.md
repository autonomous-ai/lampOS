# Self-test report names: host verification

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

## Checks

From `lampOS/`, using Rust 1.96.0 on x86-64 macOS:

```sh
cargo fmt -p lamp-cloud --check
cargo test --offline --locked -q -p lamp-cloud --lib reports::
cargo clippy --offline --locked -q -p lamp-cloud --all-targets -- -D warnings
TMPDIR=/tmp cargo test --offline --locked -q -p lamp-cloud
```

The focused report suite passed **70 tests**, with one existing ignored test.
Strict all-target Clippy and formatting passed. The complete crate gate passed
**590 tests**: 542 library, one binary and 47 integration tests; four existing
tests were ignored across the library and documentation suites. Builds used
two compiler jobs and an existing dependency cache.

The initial focused run could not bind local fixture sockets inside the
restricted sandbox. A rerun with local socket access passed. The first full
crate run then reached three macOS Unix-socket path-length failures at
106–107 bytes under the host temporary directory. Invocation-scoped
`TMPDIR=/tmp` resolved that cause without a source change. The failed logs were
retained; neither failed attempt is counted as a pass.

The two new tests prove current producer names and legacy names survive the
projection, unknown failures remain counted, and canary part names, reasons
and reading strings are absent. A report containing every supported failure
and every diagnostic part fits the existing 4 KiB limit with the longest
permitted device ID. Repeated producer entries that exceed the wire limit are
still rejected and counted as too large.

The production change adds nine fixed strings to the existing lookup. It adds
no input callback, I/O, thread, scheduling or network operation. No interactive
latency improvement or target-lamp timing was measured.

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
