# Mood input controls on pinned 99b

The [retained full a364 failure](../tests-mood-a364-failure-20261027/) stays **337 PASS / 3 FAIL**, Cargo 101/controller 1. These controls do not waive it or explain the distinct missing-waking signature.

The separate unmodified 99b runtime driver passed **28/0/0 once**, direct exit 0/controller 0. It restored the exact candidate source. This unmatched observation does not prove no regression or equivalent host load.

A private diagnostic inserted one 800 ms pause after successful Room publication, before Voice publication, in the unchanged NoMood clock case. Every original assertion remained. Both pinned sources failed once with the same stale/refused assertion:

| Source | Actual pause | Typed Voice age at first service read | Result |
| --- | ---: | ---: | --- |
| Main 99b | 806,325 µs | 607,000 µs | 0 PASS / 1 FAIL, direct 101 |
| Candidate a364 | 808,161 µs | 610,000 µs | 0 PASS / 1 FAIL, direct 101 |

The real typed-input freshness budget is 500,000 µs. Both traces have one refused input and zero input errors. Publication wall samples bracket a recent Voice envelope; its header was not directly sampled. The observations demonstrate a shared failure mechanism under this controlled pause, not the cause of the earlier full-run failures.

The original pair controller remains **CONTROL_FAIL/native 1**: a private receipt filename collision prevented its candidate arm. A reviewed continuation ran only that unstarted arm; it closed **CANDIDATE_CONTINUATION_COMPLETE/native 0**. The pair therefore consists of observations across two separate controllers. There was no base replay or case retry.

At continuation completion, all 10,677 candidate source entries were restored exactly (bytes, mode, mtime, device and inode), the official source was unchanged, all **284** inherited/new frozen witnesses and current actual executable guards passed, and owned process groups closed. Mutable target-cache restoration is not claimed. The official branch was subsequently carried separately to newer Main; this packet remains pinned to the stated sources.

[audit.json](audit.json) embeds terminal results, both observations and immutable private receipt hashes. No product code, policy, budget or device changed.
