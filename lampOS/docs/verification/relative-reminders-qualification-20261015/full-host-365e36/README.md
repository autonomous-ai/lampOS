# Relative reminder follow-up: complete Mac gate and isolated failures

Ready for lamp-voice: no

The complete affected-package gate on task head
`365e36dbcd78ff247adf30b2d18afd4d9d6f1065` **failed**:
**1,604 passed / 2 failed / 13 ignored**, 65 harnesses, Cargo exit 101.
It ran once on Third's Intel i7-10700K x86_64 Mac, macOS 26.6.2 / Darwin 25.6.0,
64 GiB RAM, Rust/Cargo 1.96.0. From `lampOS/`, with `RUST_BACKTRACE=1`:

```sh
cargo test --offline --locked -p lamp-actions -p lamp-voice -p lamp-voice-eval -p lamp-memory --no-fail-fast -- --test-threads=1
```

The command ran from 2026-10-14 22:06:17.892286 to 22:34:26.806898 UTC,
1,688.814767791 seconds measured by a monotonic clock from process launch
through exit and log closure. Its 7,200-second outer budget did not expire.
These are process timings, not interaction or lamp latency. It started after
the independent-copy worker released Cargo; no task-owned local Cargo work
overlapped it.

Both failures are unchanged Voice motor fixtures. Each panicked at the
line-441 wait assertion, `owned child timed out: None`:

| Test suffix under `coordinator::motor::tests::` | Full failure phase | One isolated result |
| --- | --- | --- |
| `crash_and_stalled_control_never_allow_replacement_or_false_cleanup` | `OptionalMotor::reaped` predicate at `motor/tests.rs:645`; crash/hang iteration not recorded | **Failed**, 0 / 1 / 0, exit 101; same wait/caller; 4.124137126 seconds |
| `late_started_callback_cannot_restore_an_offer_after_owner_replacement` | Initial `OptionalMotor::is_ready` predicate at `motor/tests.rs:865`, before the replacement assertions | Passed, 1 / 0 / 0, exit 0; 3.102594479 seconds |

Each isolated command used the original compiled test binary directly,
with the full test name, `--exact --test-threads=1`, `RUST_BACKTRACE=1`, and
working directory `lampOS/crates/voice`. No rebuild or second isolated attempt
was made. The [isolated record](isolated-first.json) gives exact commands.
The binary was 43,522,696 bytes, SHA-256
`2baf1c79df5796b498857231db44e5ae5c4627d84b5f80f02b71ee7985f63117`,
identical before the full gate and before/after isolation. The fixture source
matches integration `cf2f5b43c8113a704324c2743af3cd92bbc4fdda` byte for byte;
the task changes no Voice source. All 711 frozen implementation hashes stayed
unchanged.

[First's existing condition](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386)
allows an untouched full-gate failure that passes alone to be listed in a ready
PR with #739. **It is not satisfied here:** one test also failed alone.
The follow-up stays not ready. Fourth owns these fixtures under #739. No cause
is inferred from the timeout, and no fixture, assertion or deadline was edited.
The separate [green native ARM64 run](../native-365e36/README.md) does not
relabel this failed Mac gate. No lamp or live provider was accessed.

- [Original complete-gate report](full-first.json) and [raw output](full-first-tests.txt)
- [Original isolated report](isolated-first.json), [failed case](isolated-first-1.txt), [passing case](isolated-first-2.txt)
- [Pre-run test-binary identity](../test-binary-before-full.json)
- [Retained handoff condition](first-handoff-condition.json)
- [Parent verification and retention hashes](parent-verification.json)
