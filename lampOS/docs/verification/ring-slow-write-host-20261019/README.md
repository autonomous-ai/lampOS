# A slow SPI write no longer ends the ring, and a faulted ring comes back: host, 2026-10-19

For #1620. On lamp-52e6 the ring worker ended mid-session on `ring output
fault: SPI write returned after 13180 us; budget is 10000 us` (2 of 2
stops); the voice went on without the ring, and every later stop failed
verified cleanup on the missing Shutdown black receipt. What changed is in
[ring-choreography.md](../../ring-choreography.md), "Slow SPI writes and the
ring's restart". This page is host evidence only: nothing here ran on a lamp.

## Commands (from `lampOS/`, on the seventh builder's Mac)

Environment: `PATH=$HOME/.cargo/bin:$PATH RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`, toolchain 1.96.0, `-j2`,
`--test-threads=1`.

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings
# ring and voice for the lamp (sysroot env from ~/.local/share/lampos-sysroot/env.sh)
RUSTUP_TOOLCHAIN=1.96.0 cargo-zigbuild clippy -j2 --target aarch64-unknown-linux-gnu \
  -p lamp-ring -p lamp-voice --all-targets -- -D warnings
cargo +1.96.0 test -j2 -p lamp-ring -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 ring
cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 process
cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 bus_out
cargo +1.96.0 test -j2 -p lamp-voice --test ring_worker -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice -p lamp-ring -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

## Counts

| Gate | Result |
|---|---|
| fmt --check | clean |
| clippy, workspace, host | clean |
| clippy, `lamp-ring` + `lamp-voice`, aarch64-unknown-linux-gnu | clean |
| `lamp-ring` | 51 passed |
| `lamp-voice --lib ring` | 209 passed |
| `lamp-voice --lib process` | 39 passed |
| `lamp-voice --lib bus_out` | 20 passed |
| `lamp-voice --lib optional_ring` | 24 passed |
| `lamp-voice --test ring_worker` | 23 passed |
| full `lamp-voice` + `lamp-ring` | 2427 passed, 0 failed, 12 ignored (voice lib 2133, `ring_worker` 23) |
| build --workspace | clean |

The new tests use fake ring processes and a fake SPI sink. The sink's write
duration is injected, not timed, so host load cannot change it: in the ring
crate through its test clock, and in `ring_worker` through
`FrameSink::simulated_elapsed_us`, a test seam that real sinks leave at
`None`. The only write-time fault is a run of late writes lasting 2 s
(`LATE_RUN_US`, #911); no single slow write is one.

- `lamp-ring`: `one_late_write_is_a_late_frame_not_a_fault`,
  `an_sd_mmc_retune_of_late_writes_is_kept_not_a_fault` (fifteen 20 ms
  writes, 0.3 s, then one write of 290 ms: kept),
  `late_writes_lasting_the_late_run_are_an_observed_fault_not_a_claimed_timeout`
  (`… late for 2010 ms`),
  `an_on_time_write_ends_a_run_of_late_ones`.
- `ring_worker` (the worker's real loop): `an_sd_mmc_retune_of_late_writes_is_kept_and_the_ring_goes_on`;
  `a_late_run_past_two_seconds_faults_and_a_ring_started_again_writes_black_then_the_light`;
  `late_startup_black_write_is_reported_once_on_stderr` (the journal line, from a child copy of the test).
- `ring_worker::late_said_tests`: one line a minute at most, the unsaid counted.
- `optional_ring_tests::a_faulted_ring_starts_again_off_the_loop_and_a_stop_after_it_is_clean`:
  no restart before 2 s after the fault; every restart tick under 50 ms
  while the startup runs on its thread; the current authority (a closed
  privacy switch) reaches the restarted ring; `Rig::shutdown` then succeeds
  with the restarted ring's black-write receipt (`ring_shutdown_write`).
- `optional_ring_tests::a_stop_during_a_restart_waits_for_it_and_ends_clean`:
  the restarted ring's peer says Ready only after the stop begins, 100 ms
  late, so the stop lands while the restart's thread runs (`settle_restart`);
  `Rig::shutdown` is Ok with `ring_restarted` and `ring_shutdown_write`.
  5 of 5 run alone.
- `optional_ring::tests::a_ring_past_its_restarts_stays_off_and_says_so_once`:
  the third restart (failing to start) is the last; a fourth fault starts
  nothing, `ring_off` once, `off_for_good`.
- `bus_out::tests::a_ring_off_for_good_is_a_warning_with_its_fact_and_the_rest_after`.

## What to look for on lamp-4ace

Not run: no device was touched for this. With the build on the lamp, a
minute of normal running and then one stop of the voice (which writes the
black), `journalctl -u lamp-voice` should show:

- In normal running, usually nothing from the ring. A write the SD/MMC host
  holds up shows at most one line a minute:
  `lamp-voice ring: SPI write 13180 us, over 10000 us, late for 340 ms`
  (with `(N more since the last line)` when more came in that minute).
  A re-tune (0.3 s) should never be followed by `ring output fault`.
- If the ring does fault (`ring output fault: … late for 2000 ms` or more):
  2 s or more after it,
  `lamp-voice: the ring faulted (…); starting it again, 1 of 3`, then
  `lamp-voice: the ring is back (1 of 3 restarts)`, and the light back as the
  state says (dark with the privacy switch closed).
- The stop: no `ring shutdown has no verified black-write receipt` and no
  failed cleanup; in the session trace a `ring_shutdown_write` row.
- Only after a fourth fault in one run:
  `lamp-voice: the ring faulted again after 3 restarts (…); it stays off this session`,
  and `lamp-health show` naming `voice` with `the light ring faulted and stays
  off until the voice starts again` (fact `ring` = `off`).

Not measured: the restart's time from fault to light on the A733, and a
a real SD/MMC re-tune against the 2 s run rule.
