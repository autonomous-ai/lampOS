# Eyes failed-write fixture, 23 October 2026

Part of #1569. Frozen base `ae4ab0022`; x86_64 macOS, Rust 1.96.0.
The old test opened `/dev/full` unconditionally. On this Mac it exits 101
with `NotFound` before checking failure reporting: **0 passed / 1 failed**.

The original test now gives the unchanged `Log` a real file opened read-only.
The OS rejects its writes even when permissions or the executing user change.
It passes **1/0** with the same three assertions: the first error is reported,
the second is silent, and no line is counted. A separate Linux-only test retains
the original append-open `/dev/full` operation and those same assertions;
it does not skip or fall back if opening the device fails. The production
prefix is byte-identical, and no generic writer or runtime option was added.

## Native qualification

Commands ran from `lampOS` with an exclusively owned external Cargo target,
`CARGO_INCREMENTAL=0`, two build jobs and debug symbols disabled. All 6,515
tracked lampOS inputs were hashed before and after every command and stayed
unchanged. All source mtimes were refreshed when moving to this checkout to
avoid reusing another worktree's compiled project sources.

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --locked --offline -p lamp-eyes --all-targets -- -D warnings`:
  exit 0.
- `cargo test --locked --offline -p lamp-eyes`: **286 passed / 0 failed /
  5 existing ignored**, exit 0. Library 276/0/5; main binary 8/0; integration
  and example 1/0 each; remaining binary and docs contain no tests.
- `cargo build --locked --offline --workspace`: exit 0.

The first full-package invocation inside the restricted tool sandbox is retained:
**267/9/5**, exit 101. Eight cases could not bind local Unix test sockets;
the camera-helper case failed a local process operation, all `PermissionDenied`.
The same unchanged command was then run once with local process permissions.
Its successful library executable has the identical hash to the denied run.
This is an execution-permission correction, not a discarded flaky-test retry.
An earlier external wrapper launch could not find Cargo on PATH and started no
Cargo process; PATH was corrected before the recorded baseline command.

`native.json` records exact commands, exits, times, environment, artifact hashes
and source-map references. `raw-output.json` retains every complete stdout and
stderr string with original UTF-8 byte count and SHA-256. `sources.json` gives
the complete base input map and two-file candidate delta, including the unchanged
production-prefix and assertion-block hashes. `host.json` separates the Rust
host triple and Mach-O executable architecture from unavailable hardware and
translation queries. `manifest.json` hashes this portable receipt's files.

## What was not shown

Linux `/dev/full` execution and the PR's native ARM64/Linux checks are pending;
no ARM64 build ran on the Mac. The read-only descriptor covers a write error,
not disk exhaustion itself; the retained Linux fixture covers `/dev/full`.
There is no new lamp, camera, privacy, disk durability or product-runtime result.
