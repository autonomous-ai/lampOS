# Sensor 5 journal reader: host checks, 27 November 2026

Base: `884cf73575a78a731920c9f45ca4b83caa8d0ce9` on `lamp-voice`.
Task branch: `third/speaking-face-8848`. Machine: Darwin x86_64;
Rust 1.96.0. This is the offline journal-reader step, not the speaking-face
candidate, an accuracy score or a lamp run.

## Checks run

From the task worktree's standalone `lampOS/` root unless noted:

| Command | Result |
|---|---|
| `cargo test -q -p lamp-eyes --lib speaking::` before editing | 59 passed |
| `cargo fmt --all --check` | Passed |
| `CARGO_BUILD_JOBS=2 cargo clippy -q -p lamp-eyes --all-targets -- -D warnings` | Passed |
| `cargo test -q -p lamp-eyes --example speaking_replay` | 11 passed |
| `CARGO_BUILD_JOBS=2 cargo test -q -p lamp-eyes` outside the sandbox | 521 passed, 5 existing ignored |
| `CARGO_BUILD_JOBS=2 cargo build -q --workspace` | Passed |
| `node ../.github/scripts/docs-links/check.mjs .` | 201 top-level docs, zero broken local links |
| `git diff --check` | Passed |

The first full eyes-suite attempt in the sandbox failed 27 Unix-socket tests
with `Operation not permitted`. The complete rerun outside the sandbox passed;
no socket test was removed or ignored to obtain that result.

The same source, including the uncommitted reader, was copied without `target/`
into `/private/tmp/third-sensor5-standalone.I4FGXy`, with no parent repository or
`.git`. From that directory:

```sh
CARGO_TARGET_DIR=/private/tmp/third-speaking-face-8848/lampOS/target \
  CARGO_BUILD_JOBS=2 cargo test -q -p lamp-eyes --example speaking_replay
```

All 11 reader tests passed. The target cache was reused; no workspace source came
from the parent repository. This was not a cold dependency build.

The actual CLI was also run on a synthetic journal and two numeric line records:

```sh
cargo run -q -p lamp-eyes --example speaking_replay -- \
  /private/tmp/third-sensor5-cli-smoke/journal.jsonl \
  /private/tmp/third-sensor5-cli-smoke/lines.jsonl
```

Its JSON retained both lines, counted one without any diagnostic, reported a
largest-face `yes` in a two-face observation, and left both voice outcomes
unreported. It invented no attribution. These were synthetic input records,
not recordings or a recreation of #8791's 88 lines.

## Linux and device limits

The command

```sh
CARGO_BUILD_JOBS=2 cargo clippy -q -p lamp-eyes --all-targets \
  --target aarch64-unknown-linux-gnu -- -D warnings
```

could not finish on this Mac: a dependency's C build needs
`aarch64-linux-gnu-gcc`, which is not installed. Native ARM64 tests/build and
Linux Clippy are the draft PR's CI gates, not a local pass claimed here.

No SSH, lamp deployment, live camera or microphone use, or real-person inference
was performed. The lead confirmed that the original 2/88 session's eyes data
was not retained; a new private session export is required. The reader cannot
derive capture timestamps, track identity or actual inference latency from the
existing largest-face journal. The approved 95% and latency gates remain
unmeasured. See [the reader's contract](../../speaking-face-sensor.md).
