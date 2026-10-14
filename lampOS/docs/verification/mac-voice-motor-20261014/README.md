# Existing Voice motor fixture failure, 2026-10-14

Development Mac only: macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K
3.80 GHz, 16 logical CPUs, 64 GiB RAM, Rust/Cargo 1.96.0. No lamp or real
motor was accessed. No other local build/test was deliberately running.

Task 29 source at `4218712f3c79f96d17dfcbc1b8654c87840cafda`, based on
`a9a7262a904d802fa54043fe43cf9e368837349b`. From `lampOS/`:

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=1 cargo test -p lamp-memory -p lamp-voice -p lamp-voice-eval --offline --locked --no-fail-fast
```

Exit 101. All 60 suites completed: 1,460 passed, one failed, 11 explicitly
ignored manual/device checks. The Voice library had 836 passed, one failed,
six ignored, 437.55 seconds. The two earlier privacy-fixture failures passed
in this run. Complete unedited output is [full-tests.txt](full-tests.txt).

Failure: `coordinator::motor::tests::late_started_callback_cannot_restore_an_offer_after_owner_replacement`,
`crates/voice/src/coordinator/motor/tests.rs:896`. Reading the inert child's
`child-position-writes` receipt after observing it reaped returned ENOENT.
That file is written by the test backend's `finish`; the missing receipt
alone does not identify how the child exited. The test's motor files are
byte-identical to the upstream base; this fixture starts no Memory service
or provider. No production timeout, safety limit, assertion or fixture was
changed. The full run is a failed gate, not a pass because other suites pass.

## Not shown

The cause, reproducibility, real hardware cleanup and motor behavior are not
established by this failure. Fourth has no permission to edit this motor
fixture and has logged it for the owning builder.
