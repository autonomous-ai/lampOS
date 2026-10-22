# Voice fixture final qualification, 22 October 2026

Part of #1569; draft [PR #3023](https://github.com/autonomous-ai/lampOS/pull/3023).
The seven assigned fixtures are test-only. This receipt records failed gates as
well as passes; it is not a claim that the native package is green.
The [original controls and earlier observations](https://github.com/autonomous-ai/lampOS/tree/141336d93e49ff65f4a5eaaea4db2fb8054f810f/lampOS/docs/verification/voice-full-run-events-20261022/)
are on the PR branch, not this log-only branch.

## Exact final-base native comparison

Both observations used native x86_64 macOS, rustc 1.96.0, default parallelism
(`RUST_TEST_THREADS` unset), offline dependencies and the same exclusive target.
The full command was `cargo test -p lamp-voice --offline --no-fail-fast` from
`lampOS/`. No repeat was made to turn either failed observation green. Host
scheduling was not controlled, so this pair does not establish causality.

| Full target | Unchanged d7492ca743 | Candidate ebbb25cb1 |
|---|---:|---:|
| Library | 3,356 passed / 0 failed / 14 ignored | 3,355 passed / 1 failed / 14 ignored |
| Binary unit | 1 passed / 0 failed | 1 passed / 0 failed |
| Integration | 260 passed / 1 failed | 260 passed / 1 failed |
| Examples | 10 passed / 0 failed | 10 passed / 0 failed |
| Doc tests | 0 passed / 0 failed | 0 passed / 0 failed |
| Command exit | 101 | 101 |

All seven assigned library cases passed in the candidate; exact identities and
log hash are in `assigned-seven.json`. Fmt, strict Voice all-target Clippy and
workspace build passed. `final-head-results.json` and `final-base-results.json`
record exact commands, durations, environment, exit codes and summary lines.
The `base` key in those runner receipts means the checked-out commit, including
when that commit is the candidate.

The candidate-only library failure is
`provider_worker::tests::kept_start::with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end`:
`worker hung`, after a 111 ms authority lapse. The unchanged full base passed
that test. It is **not established as inherited**, and no claim that the patch
caused it is made either. The test and LiveRig helpers are byte-identical to
the base; the test pauses Tokio while its authority heartbeat still uses host
time. The recorded path is consistent with host scheduling exhausting the
100 ms lease, retiring request 2 before the strict-timeout End. That is a
source-grounded explanation, not a reproduction of the original schedule.

One additional **unchanged-base diagnostic**, compiled once and executed once,
injected only `std::thread::sleep(Duration::from_millis(200))` immediately
after the original wire equality assertion and before the strict-wait loop.
The scratch-only patch is preserved losslessly. It failed the original
`worker hung` assertion (exit 101; 0 passed/1 failed/3,369 filtered), after
request 2 abandonment with all 3,200 samples already sent. No lapsed-authority
line was logged in this control. It demonstrates the same abandonment/hang
vulnerability on d749 under this injected stall, **not the cause of the
unscheduled candidate failure**, and does not erase either full-run result.
The original 2.5 s patience, strict timeout, 100 ms lease, 4 s watchdog and all
assertions remained. The external Python runner hashed all 1,376 build inputs,
refreshed identical input mtimes, compiled once, executed only the named case
with a 30 s process watchdog, and restored the scratch file in `finally`.
All source bytes/aggregate hashes and scratch clean status were restored;
candidate source was unchanged. `kept-start-*.json` preserve commands, exits,
exact output, patch and restoration hashes. Compile machine-message output,
the temporary executable and the external runner are not committed.
The shared target currently contains that diagnostic artifact; it must be
rebuilt before any future use, and no qualification uses it.

Both integration runs failed
`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
All 20 calls completed. The unchanged assertion sorts 20 intervals and checks
index 10 (upper median) **below 25 ms**. Base median was **81.141344 ms**;
candidate median **58.344188 ms**. Maxima 169.682511 and 100.166837 ms are
descriptive only, not the bound. The interval is real elapsed time around
argument construction and `Executive::call` through its return, including real
in-process RingLight/Bus publication and confirmation. Setup is outside it.
It is not a physical LED or end-to-end spoken response measurement. No latency
assertion or product bound was raised or removed.

The earlier e223 first candidate library failed three other process/scenario
watchdogs (SIGINT, SIGTERM diagnostics and second-route head movement); their
complete failed observation remains in the original proof. The later pass does
not erase them. Earlier native and ARM64 counts are tied to e223, not relabeled
as d749 measurements. Upstream added 12 and removed 11 library identities;
`inventory-change.json` records that difference. This PR changes/ignores no
existing test identity.

## Identity and lossless output

`modified-rust-identity.json` lists all eight changed Rust files and hashes.
Current PR head `141336d93` is a documentation-only factual correction over
measured code `ebbb25cb1`; its Rust diff is empty. `final-*-source.json` are the
full measured source inventories, with aggregate digests in the results.
The source snapshots were unchanged through each command. The shared target
was used exclusively and identical input mtimes refreshed before tree switches
to avoid stale artifacts from paths outside Cargo's fingerprint.

Every `*.txt.json` stores the complete original UTF-8 output, its byte count
and SHA-256. JSON escaping preserves original newlines and trailing whitespace;
these are lossless receipts, not edited test output. No executable, binary
artifact, Cargo machine-message stream, credential or person recording is kept.
No ARM64 build, SSH or device action ran on the Mac.

## Linux qualification

| ARM64 full run | Passed / failed | Inventory |
|---|---:|---|
| Before #5895, e2232b836 | 10,456 / 0 | 10,523 listed; 67 not run |
| After #5944, ebbb25cb1 | 10,457 / 0 | 10,524 listed; 67 not run |
| Required PR #5954, head 141336d93 | 10,459 / 0 | 10,526 listed; 67 skipped |

Every run has one passing leaky test. Before/after full runs each cover all four
serial shards; partition exclusion counts must not be added as ignored tests.
The full run after is a branch snapshot; required PR #5954 tested merge
`bc0f72e` onto `135f14957`, which explains its newer inventory. The PR's Rust
patch equals the measured full-after patch. Required current-head Clippy
[#1957](https://github.com/autonomous-ai/lampOS/actions/runs/39779788056) passed.
The ARM64 [before](https://github.com/autonomous-ai/lampOS/actions/runs/39770980758),
[after](https://github.com/autonomous-ai/lampOS/actions/runs/39778150685) and
[required PR](https://github.com/autonomous-ai/lampOS/actions/runs/39779788065)
metadata and raw summary lines are preserved here.

Extracting only the `lamp-voice` library binary's terminal result rows gives
**3,378 passed / 0 failed before** and **3,379 passed / 0 failed after**;
the required PR also has **3,379 passed / 0 failed**. Each count includes one
passing `LEAK` row, as nextest's aggregate does. These are ARM64/Linux cases,
not the Mac inventory. Integration is 323/0 before and 318/0 after/required PR;
upstream removed five cases. Binary unit 1/0 and examples 10/0 pass in all.
The selected original log rows include all seven assigned cases, kept-start,
and light-call; each passes in all three ARM64 runs. `*-voice-tests.json`
records per-binary counts and executed-identity hashes. The parser matches
terminal `PASS|FAIL|TIMEOUT|LEAK [duration] (ordinal/total) binary test` rows;
no skipped partition entries or progress lines enter these counts.

These green Linux observations do not erase the native failed observations.
The PR stays draft pending Reviewer-two's read/disposition and a coordinated
latest-base merge window; no merge or physical-device qualification is claimed.
