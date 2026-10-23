# Eyes failed-write fixture: terminal Linux CI

PR [#3156](https://github.com/autonomous-ai/lampOS/pull/3156), authored head
`a2da872217b20147e80e194401c5eb1300c2b24a`, on `ae4ab0022a5716b8b393956d5e26fd1480aac350`.
Both jobs checked synthetic merge `7d9908117a7b38812ab08d30111cfe6d9207e190`. Its two parents are that base
and authored head; the GitHub commit record and the local authored commit have
the same tree `12f6c7064f6cf702ed71a83d6166f54f3a659fac`. This receipt records the existing jobs; no
new run, retry, local Cargo command, source change or lamp operation was made.

| Gate | Workflow / actual job | Terminal result |
|---|---|---|
| Native Linux ARM64 | [39875582729 / 120160260567](https://github.com/autonomous-ai/lampOS/actions/runs/39875582729/job/120160260567), run 6239, attempt 1 | Both completed / success |
| Strict Linux x86 Clippy | [39875582682 / 120160259756](https://github.com/autonomous-ai/lampOS/actions/runs/39875582682/job/120160259756), run 2241, attempt 1 | Both completed / success |

The ARM job finished at 00:08:55 UTC on 23 October; workflow aggregation
finished at 00:08:56. Clippy finished at 00:04:24; its workflow at 00:04:25.
No workflow/job disagreement or PR-head drift was observed by this watcher.

## Actual scope and results

The selector chose the **whole workspace** because the diff contains
`lampOS/docs/verification/eyes-failed-write-20261023/host.json`; `PICKED` was
empty. The actual native command used release builds, profile `pr`,
`-j num-cpus` (4 cores), partition `hash:1/1`, and excluded only the two
own-harness targets for the following step:

```sh
cargo nextest run --locked --release --workspace --profile pr \
  --no-tests=pass -j num-cpus --partition hash:1/1 \
  -E 'not (binary_id(lamp-ipc::relay_signals) | binary_id(lamp-turn-end-wire::process))'
```

Nextest: **10,800 passed, 0 failed, 67 skipped**, in 187.694 seconds.
The pass count includes **one LEAK**,
`lamp-voice / same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
There are exactly 10,800 unique primary result rows (10,799 PASS + 1 LEAK).
The starting inventory reports 164 binaries and 2 skipped binaries;
144 distinct binary IDs have primary result rows. Those are different counts.
No passing rows from repeated final-output sections are counted again.

Eyes contributes **288 passes**: 277 library, 9 main binary, 1
`full_look_replay`, and 1 `example/glance_replay`. The two required rows both
passed in `lamp-eyes`, 0.004 seconds each:

- `seems_log::tests::a_failed_write_is_said_once`
- `seems_log::tests::a_full_device_write_failure_is_said_once`

Separate from nextest: **4 doctests passed, 0 failed** (Bus 2, IPC 1, Msgs 1);
`relay_signals: ok`; `lamp-turn-end-wire::process`: **12 passed, 0 failed**.
The full raw logs and summary preserve the other offline prerequisite checks.
Clippy ran `cargo clippy --locked --workspace --all-targets -- -D warnings`.
Logs identify Rust 1.96.0 with `aarch64-unknown-linux-gnu` for ARM and
`x86_64-unknown-linux-gnu` for Clippy.

The [existing native receipt](https://github.com/autonomous-ai/lampOS/blob/a2da872217b20147e80e194401c5eb1300c2b24a/lampOS/docs/verification/eyes-failed-write-20261023/README.md)
records Mac formatting, the failed baseline, the sandbox-restricted package
attempt and the later unrestricted package gate. Those observations remain.
**No separate Linux `cargo fmt` command ran**, so this receipt makes no Linux
formatting-pass claim. It does not establish a default-parallel Mac Voice
result or waive #3023's historical failures.

## Portable evidence and verification

- `raw-output.json` contains the complete ARM and Clippy job logs as lossless
  UTF-8 strings, with their original byte counts and SHA-256 values.
- The four `*-run-final.json` / `*-job-final.json` files are the complete
  terminal API responses, copied byte-for-byte.
- `3156-checked-commit.json` records the checked merge's parents/tree;
  `3156-artifacts.json` records the existing artifact's name, digest and size.
  The artifact archive was not downloaded: its digest is the ZIP's identity,
  not an independently verified hash of each binary.
- `summary.json` retains exact commands, checkout lines, all 144 primary
  binary IDs/counts, selected test rows, skip counts and separate harness
  results with raw-log line numbers. Binary IDs are nextest target identities;
  this is not a compiled test-binary byte-identity claim.
- `SHA256SUMS` covers every other file in this appendix.

From this directory, `shasum -a 256 -c SHA256SUMS` verifies all payload files.
For each `logs` record in `raw-output.json`, UTF-8 encode `text`: its byte
length and SHA-256 must equal `bytes` and `sha256`. That reconstructs the raw
files without loss. Primary nextest rows lie strictly between the one
`Starting` and one `Summary` boundary; `summary.json` gives their line numbers.
No measuring program, binary, Cargo message stream or bulk source manifest
is included in this appendix.
