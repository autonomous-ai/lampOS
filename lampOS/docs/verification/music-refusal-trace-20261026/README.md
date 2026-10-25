# Requested-audio admission record qualification

This packet records the frozen Mac host candidate at base `3d9722f38817a4dd1601fac051d36ac9f2292969`
plus the uncommitted two-file patch SHA-256 `3c4cc43ada412fc9f2e952c3496a30b4e2e94fb6ff5e026836d706c582a51e6f`.
The source manifest has 7,132 entries, SHA-256
`f0b79b0dffead9d08f530bc3060fde7cc7a3725a0fecf1f9a4466115a78d9212`. The exact two source hashes are in
[census.json](census.json). The last full-test receipt was written at
2026-10-26T04:48:25+07:00; receipt times come from file modification times,
not a measurement of fixture or device latency.

The existing `ProviderOutput::RequestedAudio` arm calls the extracted private
`handle_requested_audio`. Its admission filter, refusal precedence, preparation
and state changes, fixed journal line, seven record keys, timestamp and record
error propagation are preserved. Three tests invoke this production handler
through the existing explicit service, controlled clock and bounded fake Skills
socket. They check one finite record per handled offer, an explicitly present
accepted `refused: null`, safe fixed refusal text, strict keys with no offer
asset/digest/Skills/call identifiers, and unavailable-before-stale ordering.
The repeated offer keeps the existing one-offer refusal; rejected offers do not
start Skills work or consume the later eligible offer in the covered cases.

## Positive host gate

Formatting, strict all-target Voice Clippy and complete Voice no-run compilation
each exit zero. The focused command reports 3 passed / 0 failed. The complete
Voice command reports **4,068 passed / 0 failed /
21 ignored**, across 7 terminal summaries.
Those totals include each Rust harness/doc-test summary once. The three focused
observations are separate and are not added to the complete-suite count.

Commands ran from `lampOS/` with Rust 1.96.0, locked/offline dependencies,
one compiler job, incremental compilation disabled and serial tests:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 test -q --locked --offline -p lamp-voice --lib coordinator::tests::requested_audio::requested_audio_boundary_ -- --test-threads=1
cargo +1.96.0 clippy -q --locked --offline -p lamp-voice --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test --locked --offline -p lamp-voice --no-run --message-format=json
cargo +1.96.0 test -q --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

## Distinct negative controls and restoration

The three baseline cases pass individually through a preserved original Voice
library executable. Each mutation changes only the production handler and gets
a fresh Voice library test compilation (`fresh=false`) before its exact case:

| Mutation | Selected check | Required observed outcome |
| --- | --- | --- |
| Remove `refused` | Accepted explicit null | 0 passed / 1 failed, exit 101 |
| Add raw asset | Safe refusal and strict record privacy | 0 passed / 1 failed, exit 101 |
| Bypass outer request/session filter | Stale envelope | 0 passed / 1 failed, exit 101 |

Each reaches the declared strict record equality assertion. None is counted as
a successful control because of a compiler failure, timeout or unrelated panic.
Every command receipt confirms unchanged source during that observation.
The full source inventory is restored between mutations and at completion;
the preserved original source and executable remain hash-equal. A fresh restored
Voice library compilation succeeds and all three exact cases pass again.
Direct baseline/restored observations are separate from the complete suite.

The compact census retains commands, log/source/executable hashes, exact counts,
pre-exec witness hashes and receipt timestamps. The selected-results file retains
only literal case outcomes, summaries and assertion text. Private controllers,
full inventories, raw logs, binaries and machine-specific paths remain private;
the preparation step neither reruns tests nor replaces original failure receipts.

## Scope

This establishes the finite in-memory admission record boundary. It does not
capture or count journal output, exercise a persisted writer, establish live
provider/audio/physical behavior or measure device latency. The fixed journal
line is preserved by source extraction, not claimed as an observed journal test.
PR ARM64 and Linux formatting/Clippy CI are separate and pending at this packet.
[#2793](https://github.com/autonomous-ai/lampOS/issues/2793) remains open; this
addresses boundary coverage noted in
[#3319](https://github.com/autonomous-ai/lampOS/pull/3319).
