# Complete host gate and one isolated fixture check

The complete affected-package attempt **failed: 1,602 passed / 1 failed /
8 ignored**, across 62 completed harnesses, exit 101 after
1,732.493697235 seconds. It ran once from 2026-10-14 19:40:13.466519 to
20:09:05.960676 UTC on Third's Intel i7-10700K x86_64 Mac, macOS 26.6.2 /
Darwin 25.6.0, 64 GiB RAM and Rust/Cargo 1.96.0, with `RUST_BACKTRACE=1`:

```sh
cargo test --offline --locked -p lamp-actions -p lamp-schedule -p lamp-voice -p lamp-voice-eval --no-fail-fast -- --test-threads=1
```

`--no-fail-fast` allowed the other targets to finish after Voice lib failed;
it did not convert any failure into a pass. Voice lib reported 932 passed /
1 failed / 8 ignored in 499.36 seconds. The sole failure was
`coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`
at `crates/voice/src/coordinator/motor/tests.rs:722`: an expected Authority
packet preserving the original owner did not match. This is **not** the
earlier line-441 startup/reap timeout. The failing assertion did not record
which of four peers or which packet/owner it received, so neither is inferred.

The five [fixture/source files](../untouched-fixture-source.json) are
byte-for-byte identical at clean integration `6573ce6c1b` and offered head
`acb628b494df0c38842ea467d1289eec8d35d2ca`. Every one of the
[697 frozen source hashes](../source-frozen.json) stayed unchanged during
the complete run. Amendments during that run changed commit identity and
evidence/docs, not those runtime/manifest bytes. The terminal report records
the observed final head without relabeling when execution started.

The one exact isolated execution then **passed: 1 / 0 / 0**, with no retry,
at 20:10:58.365965–20:10:59.886428 UTC, externally measured 1.519097596 seconds.
It used the exact already-built Voice lib binary from the complete run:

```sh
cd /Users/d/code/autonomous-os/lampOS/crates/voice
RUST_BACKTRACE=1 ../../target/debug/deps/lamp_voice-a714350f33563503 --exact coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch --nocapture --test-threads=1
```

The binary was 42,967,456 bytes, SHA-256
`266c7d8a00f10a229875bf5baa431f8f7b6c1ddd804c4062859bcb0de3244427`,
unchanged before/after isolation. This digest was captured **after** the
complete run exited; no pre-complete-run digest is claimed. All source hashes
also remained unchanged. The 30-second outer process budget did not expire.

[First's #709 decision](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386)
permits an untouched test that fails in the full gate and passes alone to be
listed in the PR, with its raw run and Fourth's
[#739](https://github.com/autonomous-ai/lampOS/issues/739) repair ownership.
This observation meets that stated condition. It establishes no cause and
does not relabel the complete run as green. No motor assertion, deadline or
production code was changed. Native CI on the offered head remains a separate
required result.

The [complete report](full.json), [complete raw output](full-tests.txt),
[isolated report](isolated.json), [isolated raw output](isolated-test.txt) and
[retention index](retention.json) preserve original bytes and counts. An
independent count of anchored summary rows matches the complete report.

## Not shown

These are host process tests. They establish no lamp motion, physical privacy,
audible completion or interaction latency. The failing assertion's cause and
exact received packet remain unknown. The separate isolated pass is not a
repeat of the full gate and does not show that its ordering failure is fixed.
