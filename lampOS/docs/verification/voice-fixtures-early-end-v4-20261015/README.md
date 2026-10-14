# EarlyEnd fixture correction before loaded qualification v4

Fourth's Intel Mac, x86_64 macOS 26.6.2 (25G83), Core i7-10700K,
64 GiB RAM, Rust/Cargo 1.96.0. No lamp, cloud, hardware or real key was used.
The exact commands, UTC boundaries, durations and source hashes are in each
original `summary.json`; raw output was copied without editing.

| Focused run | Passed / failed / ignored | Test seconds | Cargo wall seconds |
|---|---|---|---|
| Shared real-provider policy and source timeline | 13 / 0 / 0 | 61.52 | 87.234 |
| Checked task cancellation and teardown | 16 / 0 / 0 | 61.53 | 71.882 |

Both ran file formatting, `cargo check --locked --offline -p lamp-voice
--tests`, then `cargo test --locked --offline -p lamp-voice --lib
coordinator::tests::early_end:: -- --test-threads=1 --nocapture`, sequentially.
Formatting and compile checks passed. Source hashes did not change during
either gate. The first run nevertheless printed a background scripted-service
`AlreadyClosed` panic during the interruption scenario. Its old task join
handling discarded that error, so the passing count was insufficient.
The raw diagnostic is retained under `before-teardown-fix/`.

All ten coordinator scenarios, eleven conversations, explicitly select the
per-instance test-only 100 ms privacy allowance, actual provider Ready before
producers, and a real 12-second conversation watchdog. Production and unrelated
tests retain 50 ms, and capture/channel age remains strictly below 100 ms.
Synthetic source samples retain their original 10 ms acquisition schedule;
late delivery cannot freshen them. The separate direct Tokio supervisor test
keeps its original flow. Both entrypoints reject a logical scenario clock
before starting resources.

The subsequent fix publishes the scripted answer's abort handle before the
current-thread Tokio runtime first yields, obtains it within the existing
three-second Ready budget, and requests cancellation before coordinator
shutdown sends provider Stop. Other peers and the shared 300 ms cleanup remain
unchanged. Both Tokio joins are checked: only requested cancellation is
accepted, while panics propagate. Three deterministic helper tests cover that
handling. The final raw run has no AlreadyClosed or integration-task panic;
its deliberately exercised helper panics are labelled and checked.

The original eleven tests and 86 assertions are retained. Assertion-comparison
records preserve the exact allowed runner-name/clock-guard/formatting changes,
and the later patch retains all thirteen prior test bodies byte-for-byte.
Independent static review found no issues with the final source hash:
`e4d70c867d7531a5e19ea50f09a4be1581e26879872eae95ef13c9cc19af7fcf`.
The tests still use sleeping process fixtures requiring forced termination;
this is not cooperative process shutdown evidence.

`frozen-source.json` identifies the later complete v4 source-only copy based
on `78b984b69fb58b42448ecb16812bc4cab668f1e5`, with 1,678 files. The task files
were unchanged across rebase onto that head. Full loaded qualification is
separate and was still in progress when this evidence was published. No
passing ten-run or native ARM64 task-head result is claimed here.

Not shown: lamp privacy response, consumption-age margins under heat/load,
acoustic or mechanical behavior, or immunity to arbitrary host starvation.
The Body ownership question remains pending and no Body source was changed.
