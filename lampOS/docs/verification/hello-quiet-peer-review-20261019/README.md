# Hello quiet peer review, 2026-10-19

Initial review of [PR #1782](https://github.com/autonomous-ai/lampOS/pull/1782),
exact head `6428774ff7d4cac9b20948df96ce79701d0b5315`. This is review
evidence, not an implementation or a ready gate. The author's branch and
review checkout were not edited. No device was accessed.

Three P2 findings were reported: a held request's decision survives a new
request; `voice/turn` is absent from the export census; and the second
route publishes its verdict while its matching transcript is still queued.
The [first two findings](https://github.com/autonomous-ai/lampOS/pull/1782#issuecomment-6261823722)
and [ordering finding](https://github.com/autonomous-ai/lampOS/pull/1782#issuecomment-6261862180)
include the reachable paths and expected behavior.

`peer-regressions.patch` adds six Policy tests and one Voice test, with no
runtime edits. It applies to a source-only copy of `lampOS/` at the reviewed
head. Apply it there with `patch -p1 < peer-regressions.patch`, then run:

```sh
cargo test -p lamp-policy --lib second_review_ -- --test-threads=1
cargo test -p lamp-voice --lib second_provider::tests::held_verdict_is_published_before_the_queued_transcript_is_consumed -- --exact
cargo test -p lamp-voice-eval --lib export::tests::the_bus_topics_hold_every_topic_the_messages_name -- --exact
```

Policy uses authored `Instant` values. The actual second-route Engine uses
the existing fake STT service and a oneshot callback barrier on its
current-thread runtime; no sleeps or network services. Its passing test
characterizes the publication order, not successful Policy behavior.
The two failing new-request cases have passing default-off controls. The
ordering failure has a passing heard-before-verdict control.

`results.json` preserves the exact commands and counts from the separate
runs. The first Policy run contained four added tests; the ordering pair
was added afterward, so their filtered counts differ. These focused counts
are not a full crate or workspace gate. With the whole patch applied,
the combined Policy selector is expected to report three passing controls
and three failing regressions on this reviewed head.

The existing export census was also the sole reported failure in native
ARM64 [run 39213394277](https://github.com/autonomous-ai/lampOS/actions/runs/39213394277):
`6799/7322 tests run: 6798 passed (1 leaky), 1 failed, 48 skipped`.
The run stopped on that failure; this is not a complete passing run.
The failed-step log SHA-256 is recorded in `results.json`.

The local runs used Rust's pinned toolchain with two build jobs,
incremental compilation disabled and development/test debug info disabled.
Generated logs, caches and binaries remain outside the repository.

## Corrected-head recheck

At `d923d46cf3b13bd14860ad1d048e0e9de31a4f7a`, all three reported
findings are addressed. A new heard request clears the inherited hold;
one request-bound pending verdict handles producer-before-heard ordering;
and `voice/turn` is counted without exporting its payload. Source loss
clears the pending verdict, and switch-off behavior retains its old quiet
accounting. No further actionable defect was found in these paths.

The original independent regressions were applied only to a new source-only
copy; the author's branch and clean review checkout stayed unchanged.
`fixed-head-regressions.patch` applies to that corrected head from `lampOS/`
with the same `patch -p1` command. Its application was checked against both
resulting test files byte for byte. All other source files still match the
pinned checkout.

With the same three commands above, the corrected head passed:

- Policy: **6 passed, 0 failed, 0 ignored, 176 filtered**.
- Producer ordering characterization: **1 passed, 0 failed, 0 ignored,
  2,053 filtered**.
- Export census: **1 passed, 0 failed, 0 ignored, 321 filtered**.

[Corrected-head results](fixed-head-results.json) preserve the exact
commands, output summaries and log hashes. These are focused peer-review
checks, not a complete crate gate. The producer result still characterizes
ordering; the six Policy tests establish that the consumer now handles it.
Native ARM64 [39217912208](https://github.com/autonomous-ai/lampOS/actions/runs/39217912208), linked to the corrected PR head,
then passed **7,376 passed (1 leaky), 48 skipped**, plus **4 doc tests** and
**12 own-harness tests**. The separate relay-signals harness, release build,
checksums and artifact upload passed. [The native record](fixed-head-arm64.json)
records that source head, complete steps and full log hash. The existing leak is
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`; inherited
example-output collision warnings are retained in the record. No device
was accessed.

## Replacement PR

[#1817](https://github.com/autonomous-ai/lampOS/pull/1817) replaces #1782
at head `48edcb04f147b1ebe33b855fbdca7f2cfb13e374`. Its full Git tree and
the reviewed corrected head `d923d46cf3b13bd14860ad1d048e0e9de31a4f7a`
are identical: `3c72796463c28af2282f7d8e2e28b5072924d991`. The corrected
source review and independent regressions above therefore cover the same
tree; the original failures and both sets of evidence remain preserved.

Replacement native ARM64
[39220766616](https://github.com/autonomous-ai/lampOS/actions/runs/39220766616)
passed **7,389 tests (1 leaky), 48 skipped**, plus **4 doc tests** and
**12 own-harness tests**. Relay signals, the release build, checksums and
artifact upload passed. [The replacement native record](replacement-head-arm64.json)
records both the linked PR head and the tested Git checkout:
`1994cad1d330635a7558b839fa859624a925916d`, merging that head into
`da0806945ec6319fc1ce13443cd46945d317bca5`.

`.github/workflows/lampos-arm64.yml` uses the default GitHub merge checkout
for PR runs; `workflow_dispatch` tests the branch head. The identical
PR-head trees therefore do not imply identical tested merge trees or test
counts. The earlier **7,376** result remains preserved for its own run.

First merged #1817 at **2026-10-19 08:32:01 UTC** as
[`9df25da794d1b7ab7bcc967aa020a63be1108b5a`](https://github.com/autonomous-ai/lampOS/commit/9df25da794d1b7ab7bcc967aa020a63be1108b5a).
Peer review of the replacement is complete. No new implementation scope
or device work was taken.
