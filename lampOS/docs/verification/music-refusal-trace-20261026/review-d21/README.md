# Requested-audio review: preparation refusal and stderr

This review packet qualifies the current base `d21a854b6a46209c8a76a70d8a8b3f3e286f0ff3`, committed
carry `beba7179b5c0293f6fb07fb132885ae41566a8dc`, plus the single uncommitted fixture patch SHA-256
`9f2f376c33ecff302131940a295c35799a800db3ce6f6ce05f2e756073d61c19`. The production handler is unchanged from
the original music PR; two added tests close the preparation-failure and exact
stderr coverage gaps. The original host and native packets remain separate.
The 7,163-entry inventory is bound by SHA-256
`56f10eb06d1e401e40260296e95994bc9c2f949130a0fa19d53c0ede8fad5bb9` and every recorded file/mode still matches
when this packet is prepared. [census.json](census.json) retains identities,
commands, source/executable/log hashes, case outcomes and rebase receipt binding.

## Current host qualification

Formatting, strict all-target Voice Clippy and complete Voice test compilation
succeed. All five boundary cases pass in the focused command. The complete
Voice suite reports **4,073 passed / 0 failed /
21 ignored**, across 7 terminal summaries.
Focused observations are separate and are not added to the complete count.
The last full-test receipt was written at 2026-10-26T07:26:53+07:00; this
comes from the receipt file modification time, not device-latency measurement.

Commands run from `lampOS/`, Rust 1.96.0, locked/offline, one compiler job,
incremental compilation disabled and serial tests:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 test -q --locked --offline -p lamp-voice --lib coordinator::tests::requested_audio::requested_audio_boundary_ -- --test-threads=1
cargo +1.96.0 clippy -q --locked --offline -p lamp-voice --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test --locked --offline -p lamp-voice --no-run --message-format=json
cargo +1.96.0 test -q --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

The preparation test occupies the real Service with an independent, ready
Ticket while the current Reply has never tried an offer. The shared production
handler records `its recording could not be prepared`, grants no Reply ticket,
consumes that turn's one attempt and rejects a repeat with its existing reason.
The independent ticket remains live, then cancellation produces its Skills
Finish and the service reaches idle. The exact trace still excludes private
asset, digest and call fields.

The stderr test invokes the exact test executable in two controlled child
modes, setting environment only on each child. A refused malformed offer must
produce the one exact fixed refusal line. An accepted offer must produce no
refusal line, after proving its accepted trace, Start, readiness, cancellation
and Finish. Successful exit and a completion witness after fixture teardown are
required in both modes. The parent owns the private child scratch, checks a
64 KiB capture limit and a 30-second diagnostic deadline, and kills/reaps before
scratch cleanup. Reads are bounded; this is no hard storage or OS teardown bound.
These are actual stderr observations, not captured systemd/journald persistence.

## Two fresh deliberate faults

The two new cases each pass through a preserved full-gate library executable.
Two distinct production-handler mutations each get a fresh compilation before
one exact case:

| Mutation | Intended check | Observation |
| --- | --- | --- |
| Change preparation-refusal reason | Exact preparation-failure trace | 0 passed / 1 failed, exit 101 |
| Change stderr refusal wording | Exact child-stderr text and count | 0 passed / 1 failed, exit 101 |

Each reaches its named equality assertion, not a compile error, timeout or
unrelated panic. The whole source inventory is restored between controls.
A fresh restored compilation passes both exact cases again. Original source
and executable copies remain hash-equal. Baseline and restored exact executions
are separate from the complete Voice suite; their subprocess harnesses are not
added as extra top-level passing cases.

## Scope and retained evidence

Raw logs, complete inventories, private controllers and executable copies stay
private; compact literal outcomes and hashes are retained here. This packet
neither reruns tests nor changes earlier receipts. Unexpected failed receipts
refuse automatic publication. It does not claim systemd or persisted journal
coverage, real provider/audio output, physical latency or lamp acceptance.
Original native evidence belongs to its earlier head; this amended head needs
its own ARM64 and Linux Clippy results. #2793's provider/lamp acceptance remains
open. Cleanup attempts cannot guarantee restoration after host failure or
SIGKILL. No real key, provider or device action ran.
