# Actions speaker-publication correction — focused evidence

Optional speaker-state failure now disables only that publisher until restart. It attempts one terminal unknown snapshot, stops optional polling/publication, retains caps observations, and keeps ordinary mute, volume and light commands serving. Caps/storage failures remain fatal.

[receipt.json](receipt.json) contains exact Rust command arguments, explicit build settings with filesystem locations redacted, stage outcomes, source/log hashes, audit and limits. Commands ran from `lampOS` on Rust 1.96.0, aarch64 macOS, with two jobs and debug information/incremental compilation disabled.

| Recorded run | Passed | Failed | Ignored | Filtered |
| --- | ---: | ---: | ---: | ---: |
| Original runtime + deterministic test | 0 | 1 | 0 | 19 |
| First corrected-runtime run | 119 | 1 | 0 | 0 |
| Final Actions suite | 120 | 0 | 0 | 0 |

The final total is 65 library + 3 executable + 29 existing integration + 23 speaker integration tests. Scoped formatting and strict all-target Actions Clippy passed. Independent review verified the frozen hashes and logs and found no P1/P2 or meaningful P3 in the correction.

[pre-fix-tests.json](pre-fix-tests.json) stores the original test-only patch as a JSON string under `patch_utf8`. Decoding that string as UTF-8 reproduces every original byte and the recorded patch SHA-256. This preserves the reproduction while avoiding literal trailing whitespace in repository files. Apply the decoded patch to the receipt's `pre_fix_head` for the historical reproduction. It compiled and failed at the intended post-heartbeat serving assertion: an empty reply before mute was issued. The patch also contains later TTL assertions that were never reached in that failed run. The final test removes those wall-clock-sensitive assertions; it keeps unchanged retained state/stamp and ordinary serving checks.

The intermediate failure was a fixture error: a new cap token reused the restored cap's issue time and correctly received `stale`. Advancing the fixture clock and issue time by 1 ms fixed it; production code did not change between those two runs. Both failures remain recorded, not replaced by the passing run.

The separate actual-Voice-Ticket expiry regression is authored and frozen, **not compiled or run in this focused receipt**. Its exact path, test name and hash are recorded for the complete consumer gate. No native Linux, complete consumer, provider or physical-lamp pass is asserted here.

If revocation cannot be delivered, the last positive snapshot may remain admissible until its original three-second deadline. The publisher never renews it after failure. Explicit configuration/topic-construction errors remain startup errors; publisher counter exhaustion is code-reviewed but not injected. All raw logs remain preserved separately; this directory contains only the compact receipt and reproduction patch.
