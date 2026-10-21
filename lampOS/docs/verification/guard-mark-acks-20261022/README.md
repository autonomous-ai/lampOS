# Guard-mode mark acknowledgements — 2026-10-22

This is the historical mark-acknowledgement candidate. A later review found
that its final 300 ms no-text assertion remained, but an earlier 500 ms
allowance after light-on had been removed. The stronger preserved-coverage
wording below was incomplete. The [pause-observation follow-up](../guard-pause-observation-20261022/README.md)
restores that allowance as an explicit no-text check and records fresh gates.
The measurements and source hashes below still describe the earlier candidate.

PR #2625's fixture repair waits for the guard's checked mark transition before sending the next visit. It changes only [`tests/it/skills.rs`](../../../crates/skills/tests/it/skills.rs) and its [English explanation](../../skills.md); production behavior and deadlines remain unchanged. This record covers native Mac verification at candidate source based on `0566796d79c162077fd9b254956d66e3728adbb3`, with pre-amend HEAD `e441494cbac5fca451c1d20ce86a9a80c9b2723f`. It makes no Linux ARM64 or device claim.

## Completion and preserved coverage

The removed helper sent three zero-duration skips and claimed the third proved a full main-loop pass. `take_skips` drains a `while try_recv` loop and acknowledges each request inside it. Server serialization does not prevent another client request being enqueued before the next `try_recv`, so all three can be answered before the later `watch_look`. This is a source-level proof gap; no observed CI failure is attributed to it.

The replacement subscribes to `skills/events` before child startup and waits, under a single five-second bound, for `guard-mode`, `Done`, and the exact reason `its mark shows: counting visits`. After a 120 s or 600 s skip it first observes `UNCONFIRMED`, then publishes the refresh and waits for a new shown transition. It does not treat a skip acknowledgement as ring-feedback completion. The fixture isolates one guard and serializes these transitions; the event has no request ID.

All 45 original assertion macros across the four affected cases are preserved (18/8/11/8, compared with actual base). Original 300 ms `ONE_PASS` no-text observations remain. The upstream app-owner/switch-close test is byte-identical to base, including its 27 assertions. Source and assertion checks are recorded in [source-proof.json](source-proof.json).

The 30 s confirmation case restores the original **500 ms quiet drain after each of three 10 s skips**, checking every renewal's light and valid claim and retaining `renewals >= 3`. A renewal publication occurs before the worker arms its next wait, so receiving it alone does not prove that the following skip can advance that wait. The restored interval preserves the original observation allowance; it is not a claimed worker-readiness acknowledgement. This deliberately gives up part of the old timing reduction.

## Comparable timings

All 24 declared samples passed. Each duration is one compiled integration-test process from start to exit; compilation is excluded. Before uses the old PR helper at the same base, after uses this repair. Three samples per case were declared in advance and none were retried. The first observations are retained. Target: each repaired case below 4 s; all 12 after samples met it. Three before samples exceeded it. These observations do not establish device latency or a stable speedup.

| ID | Before, seconds (1/2/3) | After, seconds (1/2/3) | Median before → after |
|---|---|---|---|
| T1 | 6.8502, 2.0347, 2.1869 | 3.4894, 1.6207, 1.1589 | 2.1869 → 1.6207 |
| T2 | 4.4137, 2.4695, 4.0417 | 2.5751, 1.2927, 1.0794 | 4.0417 → 1.2927 |
| T3 | 3.3383, 1.4179, 2.4772 | 1.2112, 1.1328, 1.1211 | 2.4772 → 1.1328 |
| T4 | 1.5160, 0.8809, 2.2703 | 1.9532, 1.9685, 1.9730 | 1.5160 → 1.9685 |

The exact test names, sample commands, all durations, exits and raw-output digests are in [results.json](results.json): T1 texts a visit/ends on the asker's face; T2 counts only while its mark shows; T3 pauses under the person's light; T4 counts nothing after 30 s without ring confirmation.

## Controls and final gates

Both replayable Rust patches change the actual `Watch::mark_shown` acknowledgement path. Each compiled and failed exactly one selected integration test, with 0 passed/1 failed/0 ignored and exit 101:

- [Ignore initial mark](ignore-initial-mark.patch): the bounded wait fails at `initial or re-asked mark` (17.6539 s command wall time).
- [Ignore refreshed mark](ignore-refreshed-mark.patch): initial shown and expiry are observed, then the bounded wait fails at `refresh after expiry` (17.3707 s command wall time).

Control command times include Cargo compilation. [failures.txt](failures.txt) retains both complete merged stdout/stderr streams and the complete earlier full-gate failure. Patches apply independently from the repository root. Replay one patch at a time in a disposable checkout, run the command below, then restore `watch.rs` before another control or any gate:

```sh
git apply --check lampOS/docs/verification/guard-mark-acks-20261022/ignore-initial-mark.patch
git apply lampOS/docs/verification/guard-mark-acks-20261022/ignore-initial-mark.patch
(
  cd lampOS
  cargo test -q -p lamp-skills --test it skills::guard_mode_texts_a_visit_on_messages_out_and_ends_on_the_askers_face -- --exact --nocapture
)
```

The Cargo command uses `lampOS` as the working directory; exact arguments for every command are in the results. Use the refreshed-mark patch for the other control. Never commit a mutation.

| Command / stage | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all -- --check` | passed | 10.4007 |
| `cargo clippy -q -p lamp-skills --all-targets --message-format=short -- -Dwarnings` | passed | 50.0706 |
| First `cargo test -q -p lamp-skills` | **failed**: library 133 passed, 4 failed | 126.3606 |
| Actual-base `cargo test -q -p lamp-skills --lib` | 137 passed | 2.5045 |
| Restored candidate `cargo test -q -p lamp-skills` | **417 passed**, 0 failed/ignored (137 library + 19 binary + 261 integration; 0 doc tests) | 62.3346 |
| `cargo build -q --workspace` | passed | 58.0511 |
| `git diff --check` (repository root) | passed | 0.4937 |

The first full gate failed in four unchanged `routine` library tests: future wall sample, delayed wall sample, trusted correction, and episode-cap cancellation. Each unwrapped a missing request; exact names and source locations are in the full failure output. The cause is not established. That run stopped at the library and did not reach the full integration suite.

The predeclared follow-up overlaid only the two changed files from **actual base**, confirmed the entire tracked tree equaled base, and ran its whole library once with the recorded settings and default concurrency. After exact candidate restoration it ran the full Skills command once. Both passed; the earlier four failures therefore remain **unreproduced on actual base**, not diagnosed as inherited. The library executable SHA-256 was identical before/after base and after final: `ebbe6595332e5421fe1a2a665299aeef6ba1aba42752116bc30f9264b8a31cc0`. This identity covers the passing comparison; the earlier failing run did not record an executable hash.

## Reproduction and record boundaries

The compiler was `rustc 1.96.0 (ac68faa20 2026-05-25)`, native `x86_64-apple-darwin`. Both runs used the same recorded Cargo settings: `CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target`, jobs 4, dev/test debug 0, incremental 0, offline true, color never, `RUST_BACKTRACE=0`, and `/Users/autonomous/.cargo/bin` first on `PATH`. No ARM target or `RUST_TEST_THREADS` was set. The old record did not preserve every inherited variable, so full environment identity is not asserted.

For the timing samples, compile with `cargo test -p lamp-skills --test it --no-run --message-format=json`, then run the returned `it` executable with each exact test and `--exact --nocapture` three times. The before fixture is `git show e441494c:lampOS/crates/skills/tests/it/skills.rs`; use the candidate for after. Restore exact bytes between variants. This is a reproduction description, not an additional run.

Before each run's first Cargo command, 1,323 tracked crate/root Cargo/toolchain files had their mtimes refreshed, with identical aggregate content hashes before/after. The hash algorithm is sorted path, NUL, big-endian u64 content length, then exact bytes. Both mutants and both fixture variants were restored exactly; all 30 frozen source/config/doc hashes matched at the end. The follow-up also checked the complete tracked crate/root aggregate after both restorations. Only the subsequent evidence files were added after these code gates.

Bulk successful output, Cargo JSON, complete source inventories, and measuring scripts remain outside the repository under `/private/tmp/tests-codex-guard-review-20261022/`: `run-20261022T003809.369932Z` and `actual-base-20261022T005333.699975Z`. The [results](results.json) record exact command arguments, exit status, time, byte counts and raw log digests; [source-proof.json](source-proof.json) records relevant source hashes, aggregate/restoration proof and external-manifest digests. These scratch files are local records, not portable repository attachments. The Python measuring scripts remain external. No earlier reviewed evidence was modified.
