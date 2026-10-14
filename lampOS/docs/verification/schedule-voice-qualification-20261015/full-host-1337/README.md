# Complete host gate and isolated motor test

The complete affected-package gate **failed: 1,764 passed / 1 failed / 13
ignored**, 77 harnesses, exit 101. It ran once from 2026-10-14 20:42:01.057799
to 21:15:34.194745 UTC, taking 2,013.072357316 seconds from process launch
through exit and log closure. Host: Third's Intel i7-10700K x86_64 Mac,
macOS 26.6.2 / Darwin 25.6.0, 64 GiB RAM, Rust/Cargo 1.96.0.

```sh
cargo test --offline --locked -p lamp-actions -p lamp-schedule -p lamp-voice -p lamp-voice-eval -p lamp-memory --no-fail-fast -- --test-threads=1
```

The sole failure was
`coordinator::motor::tests::crash_and_stalled_control_never_allow_replacement_or_false_cleanup`.
At `motor/tests.rs:645`, it waits for `OptionalMotor::reaped`; the shared wait
assertion at line 441 timed out with `owned child timed out: None`. This is a
**reap wait**, not the readiness predicate or the earlier line-722 authority
assertion. The test iterates `crash` and `hang`; the failed iteration was not
recorded. No cause is established. Voice lib reported 947/1/8 in 508.26 s;
the remaining targets completed under `--no-fail-fast`.

The exact same compiled test was run **once alone** and passed **1 / 0 / 0**,
exit 0, in 5.000996141 seconds. The outer 30-second budget did not expire.
No source, assertion, deadline or fixture was changed. The entire motor test
file is byte-identical to integration `1337e7e1d8` and the task commit. The
Voice test binary's SHA-256 before the full run and before/after isolation is
`7759d868c24c2f83ea512b3eabcfd2bcd0dba94e98f9a32a27e77bc82fbd550f`.

[First's existing #709 condition](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386)
permits this untouched full-gate failure plus isolated pass to be listed in
the PR with its raw run and [#739](https://github.com/autonomous-ai/lampOS/issues/739).
That permits handoff; **the complete gate remains failed**. Fourth owns the
fixture work. No retry series, waiver of a new failing assertion or production
motor change is hidden in this result.

[Full report](full.json), [full raw log](full-tests.txt), [isolated report](isolated.json),
[isolated raw log](isolated-test.txt), [pre-run binary identity](binary-before-full.json)
and [retention index](retention.json) preserve the exact observations. The
single [late phase inspection](phase-observation.json) found that the gate had
just finished; it is not an additional execution or a stall diagnosis.

All 700 frozen source hashes remained unchanged at task head
`d794cf2aca8fd5a7891af6363f2155e9fd3091fe`. The native ARM64 run for that head
passed separately; it does not relabel this host failure.

## Not shown

No lamp, real provider or physical audio was used. These are test/process
durations, not acoustic latency. The isolated pass does not establish why
the full run failed or demonstrate device behavior.
