# Skills starts and answers with a full journal

Base: `a8e0c83febdb0e428f96e5afdc4fd7b842993984`. The prior journal
change rebased cleanly with range-diff `=`. Its runtime is unchanged:
`lamp_ipc::log_relay::guard()` is the first operation in Skills' main.
Only the real-process journal test and its documentation gain coverage.

## A full stream before startup

The fixture fills an isolated Unix stream before starting Skills, requiring
a one-byte write to return `WouldBlock`. Preparation alone uses nonblocking
mode; blocking mode is restored before the child starts. The peer stays
open and unread. A unique temporary folder excludes stale readiness files.

The real service must publish its tools, answer a valid `wave` call, return
the exact unknown-tool refusal, then answer another valid call and remain
alive. After it is killed and reaped, a nonblocking one-byte write must still
return `WouldBlock`; that mode change cannot help the earlier child replies.
No journal drain, bulk tool flood, device, voice or provider is involved.

## Control and observation

Removing only the relay guard gives **0 passed / 1 failed**, exit 101:
Skills does not publish its tools within the existing twenty-second startup
wait. The main source is restored byte-for-byte. The guarded module then
reports **3 passed / 0 failed**, exit 0: broken stream, full stream, and
normal CLI usage-error output.

The full-journal run filled **8,192 bytes**. Preparation through publication
of tools took **16,124 microseconds**; connect through complete reply took
**132 microseconds** for the first valid call, **93** for refusal, and **79**
for the next valid call. These are one Mac observation, not device latency,
latency targets, or process-only startup measurements.

Preparation is capped at two seconds and four MiB. Startup waits at most
twenty seconds; socket read/write timeouts are at most two seconds, capped
by the remaining checked thirty-second scenario deadline. The deadline is
checked rather than enforced by a hard total-runtime watchdog: connect,
fragmented reads and cleanup are not asynchronously interrupted by it.

[control.json](control.json) keeps exact commands, test summaries, source
hashes, raw-log hashes and the failed initial fixture attempt. That attempt
used only per-send `MSG_DONTWAIT`; it stalled before child startup in the
Mac socket send and was terminated after 199.989 seconds. Cargo returned
101 with no test summary. This is a fixture failure, not the expected
old-runtime failure, and is retained separately from the corrected control.

## Current Mac gate

The complete Skills-only command passed once after the corrected control:

- `cargo +1.96.0 fmt --all --check`: exit 0.
- `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings`: exit 0.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-skills -- --test-threads=1`: **389 passed / 0 failed / 0 ignored / 0 filtered**, exit 0.

[mac-gate.json](mac-gate.json) retains all four test summaries, exact commands,
source identity and log hashes. The Rust sources/manifests/lock files stayed
unchanged throughout. This passing run does not replace the earlier
387/1 result; its then-failing unplug fixture is unchanged in this PR.
ARM64 and x86 qualification comes from this revision's native PR runs.
No full local workspace gate or local ARM64 build was run.

## Earlier evidence and limits

The [original broken-stream evidence](../skills-journal-20261021/README.md)
retains its **387 passed / 1 failed** Mac gate and the exact isolated
comparisons. The original head's [native qualification](https://github.com/autonomous-ai/lampOS/tree/76e8398ec6bdb2b640e7d43ff1da41563e5d3f6d/lampOS/docs/verification/skills-journal-native-20261022)
qualifies that head only. Neither result qualifies this added test.

The IPC relay's internal drop accounting, saturation recovery, signal
handling and flush ceiling keep their existing IPC tests; this addition
does not duplicate or newly measure them. No lamp, sound, physical behavior
or later integration base is qualified by the local controls.
