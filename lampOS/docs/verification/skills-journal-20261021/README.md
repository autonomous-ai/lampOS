> Historical record for the original broken-stream change. The later
> [full-journal startup regression](../skills-journal-backpressure-20261022/README.md)
> has separate controls and qualification. All original counts below remain.

# Skills survives a broken journal

Base: `f349d63c7935dd16decc0badd4dd4e18c10094eb`. Only Skills changes;
no device or audible playback is used. The runtime change installs the
existing IPC log-relay guard before argument handling. The IPC relay itself
is unchanged.

## Old-runtime control and fixed behavior

The new real-process test first gets a valid tool reply and a healthy
unknown-tool refusal. The refusal fences the first call's trailing diagnostics.
It then shuts down the temporary stderr socket and requires both the exact
unknown-tool refusal and another valid reply.

With the exact baseline `main.rs`, the test **fails: 0 passed, 1 failed**,
exit 101. The post-break call receives the listener's generic
`My skills did not answer in time.` instead of the required refusal.
With the guard restored, both new tests **pass: 2 passed, 0 failed**, exit 0.
The second test checks a normal CLI usage error: complete stderr and exit 2.
The first control supervisor expected EOF; its overly narrow check stopped
after source restoration. The original observed failure is retained and was
not rerun to replace its result.

[control.json](control.json) records the exact commands, source hashes,
summaries and raw-log hashes. One fixed run measured connection start through
complete reply: **624 microseconds** for the first valid call,
**159 microseconds** for the broken-journal refusal and **198 microseconds**
for the next valid call. These are Mac IPC observations, not device or
acoustic latency. The fixture allows 2 seconds per read/write and 20 seconds
for startup/CLI exit; it always kills and reaps its child. Journal saturation,
drop-count recovery and the 500 ms flush ceiling are existing relay behavior,
not separately measured by this change.

## Mac gate and retained failure

- `cargo +1.96.0 fmt --all --check`: passed.
- `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings`: passed.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-skills -- --test-threads=1`: **387 passed, 1 failed, 0 ignored, 0 filtered**, exit **101**.

The one failure is in unchanged
`skills::unplug_tool_returns_before_folding_and_says_ready_only_after_render_receipt`:
`crates/skills/tests/it/skills.rs:11031` reads the accepted say socket and
gets macOS `WouldBlock`. Both new journal regressions passed in the full run.
The failed full-gate result is preserved in [mac-gate.json](mac-gate.json),
with all four summaries and source identity.

One exact comparison per source then passed on the baseline runtime and on
the guarded runtime: **1 passed, 0 failed** each. These focused results do
not replace the failed full gate or establish its cause. The fixture's
nonblocking listener is unchanged; its accepted-stream race is being read
separately. [unplug-comparison.json](unplug-comparison.json) retains both
commands and log hashes.

The PR's native Linux ARM64 build-and-test run and x86 Clippy decide the
remaining platform qualification. No full local workspace or local ARM64
gate was run.
