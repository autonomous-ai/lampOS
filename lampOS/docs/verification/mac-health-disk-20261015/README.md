# Mac Health fixture depends on the host disk warning

Ready for lamp-voice: no

The full Mac workspace gate at Memory source
`10e4ba509f2d1cbb97a52979c09deb0d2d1a371e`, based on
`7fbb03e1f2e61ad6dfaf184cd768bad6524c2d76`, failed one completed suite
and was then interrupted. At 00:13 ICT on 2026-10-15 its tool handle no
longer existed; a targeted process inspection found no gate or test
process. The saved output ended mid-way through Settings' release suite.
The interruption's cause and final process exit status were not available.
The log contains 166 completed Cargo suites: **2,257 passed, 1 failed,
16 ignored**. Later suites and the workspace build were not completed.
This is not a passing gate.

Machine: macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K, 64 GiB, Rust
1.96.0. No lamp or external provider was involved. With four build jobs,
incremental compilation disabled and one test thread, formatting and
strict workspace Clippy passed before:

```sh
cargo test --workspace --offline --locked --no-fail-fast -- --test-threads=1
```

Cargo warned that Controls and Expression both produce the example
`replay_cost` (including its Mac dSYM output). This pre-existing naming
collision was not edited or suppressed. It was a Cargo test-build warning,
not a strict Clippy failure.

## Found, not mine

`crates/health/tests/program.rs:536` failed
`a_part_that_is_expected_and_silent_or_has_gone_quiet_ends_show_with_3`:

```text
left: "lamp-health: health warn: board: the disk is filling up: 9% of it is free"
right: "lamp-health: health stale: eyes: has never reported"
```

The fixture's proc/sys files are synthetic, but its disk measurement is
real: `tree/mod.rs:147` supplies a directory on this host's filesystem;
`board.rs:420-430` reads `statvfs`. Its warning threshold is 10% free
(`board.rs:47-50`). The Health reducer prioritizes an existing non-OK item
over a missing part (`parts.rs:190-198`). Earlier assertions in this same
test use the measured board level (`program.rs:185-214`, 500-522), but
the final journal assertion unconditionally expects the missing Eyes
message. The program's disk warning matches its implementation. The
fixture should account for that board level in its journal assertion too.
All four inspected files are byte-identical to the upstream base; their
hashes are in `source-sha256.json`. Fourth did not change them.

## Environmental repair and focused retest

After confirming the interrupted gate was no longer running, Fourth
removed only its inactive `/private/tmp/lamp-fourth-mood/lampOS/target`
build cache. Task source and verification files were preserved. Recorded
`statvfs` free space rose from **8.8804% to 12.4454%**, reclaiming
17,823,891,456 bytes; exact readings are in `cache-cleanup.json`. No
assertion, Health threshold, source file or runtime permission was changed.
The unchanged fixture then passed **6 / 0 / 0 ignored**, exit 0:

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=1 cargo test -p lamp-health --test program --offline --locked -- --test-threads=1
```

This clears the environmental condition for a fresh gate; it does not
repair the fixture's behavior on disks below 10% free or turn the
interrupted run into a pass. A fresh full gate is required.

## Raw evidence

`full-interrupted.txt` is the unmodified partial gate output (SHA-256
`9cd9f998186fe69f05f5a6b12abb500f9eeb018068b7744094f79505023ecc87`);
`results.json` counts completed result lines only. `failure-excerpt.txt`
is explicitly an excerpt. `focused-after-cleanup.txt` is the complete
unchanged-test retest output. Preserve raw whitespace, including Cargo's
final blank line and the excerpt's partial next-line indentation; exclude
these three text artifacts from whitespace normalization.
