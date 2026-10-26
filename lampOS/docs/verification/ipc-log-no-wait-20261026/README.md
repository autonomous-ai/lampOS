# IPC: full-destination fixture without a stopwatch

Part of #1339. Base `c5c7603a743690e291f85a664a7ee18624232581`, plus the two-file uncommitted patch bound
in [results.json](results.json). Only the IPC test module and logging docs change.
The complete production prefix before `#[cfg(test)]` is byte-equal to the base.
All 7,160 private source-inventory entries and modes match after controls.

The existing full-pipe/socket test runs in a child. Nothing drains its destination
before `write_now` returns. The parent requires the exact selected case and its
passing summary, not merely exit zero. A stuck child meets a 15-second diagnostic
watchdog; cleanup kills and reaps it. Existing EAGAIN and owner-blocking checks
remain. The 100 ms scheduling-sensitive stopwatch is removed. No runtime API,
nonblocking policy, message, service or default changes. The existing Polled-pipe
race with another writer is unchanged and is not covered by this fixture.

## Host gate and scope

Mac formatting, strict all-target IPC Clippy, compilation and full `lamp-ipc`
tests pass: **26 ordinary tests / 0 failed / 0 ignored**, **1 doctest / 0 failed**,
and the separate **`relay_signals: ok`** marker. Scope: eight library tests;
probe (3), relay_exit (1), relay_shared (1), and sockets (13) integration tests;
the probe binary (0), relay_signals custom harness, and one doctest. The focused
**1/0** baseline is separate and is not added to the full count.

Commands and exact selectors are recorded in results; `${CARGO_TARGET_DIR}`
and `${RECEIPT_DIR}` replace private filesystem roots. Executable hashes bind
those references. The full source inventory remains private, represented by its
digest rather than copied into this compact packet.

## Controls and retained unsuccessful attempts

| Observation | Result | Interpretation |
| --- | --- | --- |
| Original fixture plus 400 ms host pause | 0/1, exit 101 | Reaches original 100 ms assertion |
| Corrected fixture plus the same pause | 1/0, exit 0 | No stopwatch failure |
| First small blocking-write fault | 0/1, exit 101 | Accepted `Ok(14)`; rejected as watchdog qualification |
| Corrected oversized blocking-write fault | 0/1, exit 101 | Reaches the parent 15 s watchdog |
| Exact restored source, freshly compiled | 1/0, exit 0 | Original library hash and source restored |

The first blocking fault did **not** stall as intended. Its child failed the
EAGAIN equality check; the parent failed with that child output. Two echoed
failure summaries made the strict controller reject the log. This remains an
unqualified observation, not a successful watchdog control or two independent
cases. The continuation uses a 1 MiB blocking write and reaches the named
watchdog at the first blocked destination; no independent per-destination fault
coverage is claimed. Every experimental library artifact was freshly compiled.

An initial continuation script also had a syntax error before execution. Its
retained bytes still fail static parsing at line 37; no Cargo/test observation
is counted. Its retained exit-1 parse-phase receipt explicitly records no Cargo
or test execution. The corrected controller, initial controller, rejected fault,
raw logs, source inventories and preserved binaries remain retained privately.
The continuation does not overwrite or relabel earlier unsuccessful attempts.

No Linux/ARM64 result is included yet. These host checks establish neither lamp
latency nor persisted journal delivery or device acceptance. Watchdogs and output
limits are diagnostics, not hard storage or OS teardown bounds. The outer
60-second controller did not time out; a controller timeout or compile error
would not qualify as the intended failure. Raw private evidence requires
separately arranged access; hashes do not promise a public binary download.
