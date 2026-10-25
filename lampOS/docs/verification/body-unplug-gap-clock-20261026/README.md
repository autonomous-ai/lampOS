# Body unplug-gap fixture qualification

Native Mac gate on **26 October 2026, 00:34:59–00:39:01 +07**, at base
`9e9e6733a1ed32ba1a14b6920f7120035b89396a`. Rust 1.96.0,
`aarch64-apple-darwin`, one compiler job, incremental compilation disabled,
offline/locked dependencies and serial tests. The private controller exited
zero. No test was rerun while preparing this record.

## Source and behavior

This executes the previously unrun
[corrected source proposal](https://github.com/autonomous-ai/lampOS/tree/7964c07fc/lampOS/docs/verification/body-unplug-gap-source-20261025/candidate-with-live-worker).
The gap case drives production `Shared::tick` and the real kept bus on one
controlled timeline: 20 ms samples, the actual worker `PERIOD`, two 500 ms
proofs, Ready/Folding/Ready/unsafe-Failed transitions, the latch at each
observed step, two-second logical/host bounds and the moved-joint reason.
The separate enabled-Adapter case exercises actual sample encoding, worker
publication, stop, join and cleanup. Runtime and all other existing test
bytes remain unchanged in the submitted candidate.

Candidate `unplug.rs` SHA-256:
`e82d160ea17c64c49c55ac34774a6007c4d90c5052d2b4e6be5a6e941d6c79e7`.
The two-file patch captured before/after the base carry and by the gate is
identical, SHA-256
`449d9ffb363c3102b18d0432d68b0aa3b89ef60f977f78ca03dd4876eb270be1`.
The carry snapshots were recorded at 00:29:54/55 +07; the gate's candidate
snapshot at 00:34:59 +07. These times come from evidence-file modification
times, not a measurement of fixture or device latency.

## Positive gate

Commands ran from `lampOS/`, with an exclusive target and a separate owned
temporary directory for every command:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline --locked -p lamp-body --all-targets -- -D warnings
cargo +1.96.0 test --offline --locked -p lamp-body --no-run --message-format=json-render-diagnostics
cargo +1.96.0 test --offline --locked -p lamp-body --message-format=json-render-diagnostics -- --test-threads=1
```

Formatting, strict Clippy and complete test compilation exit zero. Between
compilation and the full run, the actual compiled library test executable
was launched once through the pre-exec witness runner with:

```text
unplug::tests::a_gap_in_samples_withdraws_ready_and_only_an_unsafe_one_fails --exact --nocapture --test-threads=1
```

That direct observation is **1 passed / 0 failed / 440 filtered**. The
complete Body command is **609 passed / 0 failed / 0 ignored**, with eight
summaries: **441, 32, 2, 5, 3, 1, 125, 0** passed. The last is the empty
documentation-test summary. Both the controlled gap case and
`the_enabled_adapter_publishes_its_latch_and_joins_after_stop` are literal
`ok` in the full run. The direct case is separately counted, not added to 609.

All seven Body test executables were rebuilt in the no-run stage
(`fresh=false`). That stage has 58 compiler records: 41 rebuilt and 17
reused after Clippy. The later full command reports all 58 records
`fresh=true` and executes exactly those seven previously bound test
artifacts. Each launch checks SHA-256, size and executable mode immediately
before `execv`; the recorded source-manifest digest matches compilation.
No claim that every dependency rebuilt is made.

## Five distinct negative controls

Each mutation used its own newly created target, fresh library compilation
with the no-run command above plus `--lib`, and one direct test observation.
The first three select the gap case; the last two select the enabled-Adapter
case, with the same exact/nocapture/serial arguments.

| Mutation | Observed intended assertion |
| --- | --- |
| Worker `PERIOD` becomes three seconds | First proof: no Ready within the two-second logical bound |
| Proof threshold becomes literal 100 ms; the 500 ms oracle stays | First Ready arrives at 100 ms and fails the minimum-proof assertion |
| A sample gap becomes permanent failure | Failed is rejected while waiting for Folding after the gap |
| `Adapter::sample` becomes a no-op | Safe evidence bit is 0 instead of 1 |
| The spawned worker omits its tick | No Folding publication within two host seconds |

Each exits **101**, reports **0 passed / 1 failed / 440 filtered**, and
matches its declared assertion message and source line. None hit the
20-second outer watchdog, failed compilation or required another sample.
All five mutation targets have 44 compiler records, all `fresh=false`.
The mutations are restored after each observation; none is submitted.

## Binding, cleanup and scope

All 7,075 source entries remain unchanged within each of the 16 command
observations. After every mutation and at completion, the full source
manifest returns to SHA-256
`ccdba309a61ab241c5ca374ac9b358198a1fe2583559d35e1201008fd8b425e9`.
Every owned process group had already closed when cleanup began, and every
owned temporary directory was removed, including failure leftovers.

[census.json](census.json) retains exact commands with private executable
paths replaced by explicit placeholders, source/compiler/executable hashes,
pre-exec timestamps, per-command results and cleanup checks.
[selected-outcomes.txt](selected-outcomes.txt) retains the relevant literal
test summaries and assertion messages, with thread identifiers omitted.
The controller hash is
`be2ad1c0bdc976afecd87d014d2b32dc1896a967f146e0dd7ebbe4a4795e5a1e`;
the witness runner hash is
`2a7b5d2224ad4f609e3fb87fb5072b20bacc581b2861b0313b8b9e55e1892be8`.
Private controllers, full source inventories, absolute paths and binaries
are not part of this public record.

The original three failed observations remain failed, with their Ready
phase unresolved. No unchanged baseline was repeated, and this result
does not establish their historical cause. Controlled time and the live
worker fixture do not establish physical settling, hardware latency or an
isolated host. This is the exact recorded base's Mac gate; no local ARM64
gate or device work ran. Original PR Linux ARM64 and Linux formatting/Clippy
CI remain unrun at this checkpoint.
