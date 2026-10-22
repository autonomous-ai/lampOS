# Row 72: the early-end fixture's capture clock

Part of #1569. The target is
`coordinator::tests::early_end::the_link_dropping_after_a_continued_request_fails_it_unheard_and_the_next_has_no_note`.

A host pause after PCM capture-time mapping and immediately before `input.audio`
aged a logically fresh block past Gemini's unchanged 2,500 ms input-age limit.
The controlled baseline failed with `StaleInput` 3/3 times; the same pause passed
3/3 times with this fixture's capture mapping and scripted service sharing one
explicit acquisition clock. The pause holds request 2, sequence 1 for 3 seconds
and asserts that scenario time does not advance. It exists only in the two
scratch control patches; the repaired fixture adds no completion sleep.

The control demonstrates a capture-age defect. The original row 72 report did
not retain its provider error, so this does not establish that report's original
cause ([record](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6317406905)).

## Bases and preserved behavior

Controls and the first clean checks used
`0566796d79c162077fd9b254956d66e3728adbb3`. Final gates used
`1f92c96edfac69dd34ed8211b1518ca244a6dd69`. The four changed files, scenario clock,
provider link, transport, and entire Gemini crate were identical between those
bases before this change. The controlled candidate's four source files were
byte-identical to those used by the final gates. Other upstream Voice changes
were covered by the fresh full Voice run on the newer base.

Every original early-end test body and assertion is byte-identical, including
the continued-request lineage, `provider_failed`, no note on the next request,
2 connections, 3 starts/ends, 330 audio blocks, 1 abandoned request, 2 answers,
and 48 played blocks. Existing privacy limits, scenario limits, 2 ms steps, and
real watchdogs remain unchanged. The measured first-sound boundary remains
602 ms of scenario time; this is not a device or acoustic latency measurement.

The production mapping still uses `Instant::now()`. Other test fixtures keep
that default unless they install the test-only guard. The guard is thread-bound,
restores the previous clock on drop, and spans the current-thread runtime's
reconnect and teardown. The fixture advances its clock before each runtime
pump; clock reads neither advance it nor sample host time. Gemini's 2,500 ms
age limit and the regular dependency manifest are unchanged; only Voice's
dev-dependency enables the existing `fixture-clock` feature.

## Controlled samples

All samples are retained in execution order. Wall time measures the entire
spawned command, separately from libtest's reported duration. Exact values,
markers, exit codes, and complete log SHA-256 values are in `observations.json`
and `commands.json`.

| State | Results | Command wall seconds, in order | Libtest seconds, in order |
| --- | --- | --- | --- |
| Baseline + pause | 3/3 `StaleInput`, exit 101 | 5.4303, 3.6250, 3.6207 | 3.58, 3.58, 3.58 |
| Repair + same pause | 3/3 passed | 6.0699, 4.3048, 4.2503 | 4.22, 4.22, 4.17 |
| Repair without pause | 3/3 passed | **9.2518**, 1.2969, 1.4108 | 1.19, 1.21, 1.35 |
| Complete early-end module, without pause | 20 passed | 26.7661 | 26.70 |

The first clean 9.2518-second sample is retained without an assumed explanation.
These runs establish behavior under a controlled pause, not a speedup. All
three complete failure outputs are included as `before-pause-{1,2,3}.txt`;
each contains the exact PCM refusal and the session's `Gemini Live StaleInput`
diagnostic. Every original output byte, including trailing blank lines, is kept
at offset zero; an explicit closing marker follows it. `observations.json` and
`source.json` record each original stream's byte length and SHA-256 separately
from the complete wrapped file's SHA-256. The command log hashes refer to the
original streams. No watchdog failure is counted as reproduction.

## Replay

Use disposable native checkouts. Start the baseline at the control base above;
apply `before-pause.patch` from the checkout root. For the repaired control,
start at the same base, copy the four changed source/documentation files from
this PR, and apply `after-pause.patch`. Each patch passed `git apply --check`
against its corresponding frozen source. Set `CARGO_TARGET_DIR` to a dedicated
native target directory and unset `CARGO_BUILD_TARGET` and `RUST_TEST_THREADS`.

From `lampOS`, compile each source state once:

```sh
cargo test -p lamp-voice --lib --no-run --offline --message-format=json
```

Select the `lamp_voice` test executable from that command's compiler-artifact
record. From `lampOS/crates/voice`, run it three times with these arguments:

```sh
<lamp_voice-test-executable> --exact coordinator::tests::early_end::the_link_dropping_after_a_continued_request_fails_it_unheard_and_the_next_has_no_note --nocapture --test-threads=1
```

Reverse only the scratch pause patch with `git apply -R`, compile the clean
repair again, repeat the exact target three times, then run:

```sh
<lamp_voice-test-executable> coordinator::tests::early_end:: --test-threads=1
```

`commands.json` records every executed argument vector, working directory,
start time, exit code, wall duration, and output digest. Success summaries and
digests are retained without bulk success output. Full compile JSON and the
verification runners remain outside the repository. `source.json` records the
relevant source hashes and source-equivalence checks; the original controlled
verifier's digest remained unchanged through final verification.

## Final gates on 1f92c96ed

All commands below passed on native x86_64 macOS, sequentially from `lampOS`:

```sh
cargo fmt --all --check
cargo clippy -p lamp-voice -p lamp-gemini --all-targets --features lamp-gemini/fixture-clock --offline -- -D warnings
cargo test -p lamp-voice --offline -- --test-threads=1
cargo test -p lamp-gemini --features fixture-clock --lib --offline session::capture_clock_tests -- --test-threads=1
cargo build --workspace --offline
git diff --check
```

Voice: **3,364 passed, 0 failed, 14 ignored** (3,088 library, 1 binary,
265 integration, and 4 + 6 harness tests). Gemini capture-clock tests:
**12 passed, 0 failed**. `gates.json` records the exact commands, counts, times,
and log hashes, plus the source-freshness aggregate. All frozen sources stayed
unchanged during these gates. Linux ARM64 and device results are not claimed
by this local evidence.
