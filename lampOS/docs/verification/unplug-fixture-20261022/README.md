# Unplug fixture: accepted Say socket mode

Base: `56cfc3983571645a500f2c0d8e0271292e45cc5f`. Test-only: two lines
restore blocking mode on the accepted Say connection before its existing
two-second read timeout. Every admission, physical-proof, rendered-receipt
and readiness-content assertion is unchanged. No runtime, device or sound.
This follows the same fixture correction already merged in [#2320](https://github.com/autonomous-ai/lampOS/pull/2320).

The original Mac full-crate failure is retained in [#2692's evidence](https://github.com/autonomous-ai/lampOS/tree/54e0b5e1e0d2097b7db2b8fcbc27d6520e871089/lampOS/docs/verification/skills-journal-20261021):
387 passed, one failed. The failing unplug fixture read the accepted Say
stream at `skills.rs:11031` and got `WouldBlock`. Its isolated baseline and
journal-guarded comparisons each passed; those do not erase that failure.

## Controlled descriptor check

[accepted_socket.rs](accepted_socket.rs) was compiled once with
`rustc +1.96.0 --edition=2024` and run once on the Mac under an outer ten-second
subprocess timeout. All internal socket/channel waits use two seconds.
The source uses Darwin's `F_GETFL = 3` and `O_NONBLOCK = 4`, checked against
the installed SDK's `sys/fcntl.h`. No dependency, sleep or load generator.

The client connects but cannot write before the first read. The accepted
stream's actual flags are **6**, including `O_NONBLOCK`; that old read returns
**WouldBlock / errno 35**. After `set_nonblocking(false)`, flags are **2**.
A channel holds the writer until the reader announces its read; after release,
the complete line arrives. This proves announcement-before-release, not that
the kernel read had started. Actual flags separately establish blocking mode.
It isolates socket-mode behavior rather than recreating the original service
schedule or measuring device latency.

[control-output.txt](control-output.txt) is the raw two-line output. [control.json](control.json)
records exits, source/binary/log hashes and parsed results; the generated
binary is not committed.

## Mac gate

- `cargo +1.96.0 fmt --all --check`: passed.
- `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings`: passed.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-skills -- --test-threads=1`: **386 passed, 0 failed, 0 ignored, 0 filtered**, exit 0.

[mac-gate.json](mac-gate.json) records the exact commands, all four test
summaries, source identity and raw-log hashes. This includes the original
unplug scenario. No full local workspace or local ARM64 gate was run.
Native Linux ARM64 and x86 Clippy qualification belongs to this PR's runs.
