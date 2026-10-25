# Same-voice journal fixture completion

Part of #1569. The journal test must finish only after its worker has
returned and reaped its child. Its journal assertions still run through
`Prints::journaled`; the fixture and every journal assertion are retained.
Only test builds retain the worker handle and completion receiver.
Production layout, behavior, timing and configuration are unchanged.

## Matched x86 control

Run on x86 macOS with Rust 1.96.0 and nextest 0.9.149, on base
`86416a0d3ff5575c37b48943ada962445ec935f9`. One original observation and
one candidate observation ran, in that order, with no retries. The
original child exited naturally before the candidate was allocated.

Both used this command, with the pinned nextest executable on PATH:

```sh
cargo nextest --user-config-file none --config-file .config/nextest.toml run   --offline --locked -p lamp-voice --lib --build-jobs 1 --cargo-profile test   --profile pr --test-threads 1 --retries 0 --no-fail-fast --no-tests fail   --status-level all --final-status-level all --success-output immediate   -E 'test(=same_voice::tests::each_line_the_check_says_is_said_to_its_journal)'
```

| Variant | Exact case | Nextest test duration | Other cases filtered | Post-nextest cleanup |
| --- | --- | ---: | ---: | --- |
| Original | 1 passed, **1 leaky** | 0.934 s | 3,764 | Child remained; natural exit after 29.780 s |
| Candidate | 1 passed, **0 leaky** | 0.892 s | 3,764 | No remaining descendant; empty census in 0.099 s |

These are one observation each, not a speed benchmark or a full-suite
result. Both enumerated the same 3,765 cases. The canonical case list,
joined with newlines and a final newline, has SHA-256
`2c22b92874277f9328eb80c50417d96999ee03fb748ae1752dfea2901c4f14aa`.

The unchanged PR profile uses nextest's default 200 ms test leak timeout. No
output redirection, added settle sleep, retry, or leak-policy change was
used. An exec-only runner recorded process ownership while preserving the
test's stdout/stderr. The enclosing supervisor retained its direct
nextest session leader until descendant exit events and an empty session
census completed. Both supervisors exited zero, with no signals sent.
The original's LEAK remains a failure of fixture cleanup even though
nextest's default policy reports a successful run.

Build-produced test executable SHA-256 values, frozen before observation
and checked again afterwards:

- Original: `3ae57f64fdaf917e76cdb1399dfc6d7f7aad3ddcbf974e0be57038caecdbdd24`.
- Candidate: `90f8f45be3d73c97ef66e5d644104c15528eb15848b5ba217dbeb41431668038`.

The official nextest binary had SHA-256
`db85ec6208e74eddb330bf900eb2b7c17b5f1a278a126f2a85feae0c170d43fa`.
Full raw output, source and executable snapshots, producer records and
supervisor records are retained in the builder's private evidence
packet; binaries and thousands of filtered-case lines are not committed.
Two inventory preflight assertions initially compared unlike listing
formats; neither allocated a test observation. Comparing the actual
canonical case names established equality before the candidate ran.

## Limits

Formatting and both library-test builds passed on the matched base.
Current-base broader gates and full native ARM64 CI are reported on the
pull request separately; these controls do not establish those results.
No ARM64 build ran on the Mac, and no device action ran.

The fixture closes both channels, waits on worker completion, then joins.
Completion follows the existing child kill and wait. Panic cleanup also
keeps ownership of the join. Rust cannot cancel a thread stuck in an OS
spawn or reap, so that exceptional path still needs the enclosing test
process watchdog. The production stdout-reader thread is not claimed to
be joined by this fixture.

Evidence correction (2026-10-25): the earlier 100 ms description was
incorrect. The pinned binary's `nextest help repo-config` gives 200 ms
for tests; 100 ms appears for setup scripts and examples. Neither
variant overrode the test default. Historical private plans retain the
incorrect declarative field `leak_period_ms: 100`; the supervisor checked
that field but never passed it to nextest as configuration. This erratum
does not replace the raw results or allocate another observation.
