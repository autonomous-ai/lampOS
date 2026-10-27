# Restart a209 original native CI

PR [#3572](https://github.com/autonomous-ai/lampOS/pull/3572), pushed head `a209424d3c5f07ca3423447bc2bb8bcb6d0a18e0`, authored parent `99b5161531791de7ed57ce57f6cacf988d509f25`. Original attempt 1 runs passed: [ARM64 build/test](https://github.com/autonomous-ai/lampOS/actions/runs/40433774956) and [Linux strict Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/40433775214). No rerun was requested.

Both job logs actually checked out preview `b04d187200641edbb9d721258596409e35c69bef`, ordered parents **737331535d0cab813170d288110fc44ff5d41a68 + a209424d3c5f07ca3423447bc2bb8bcb6d0a18e0**. Its tree `747fd001a8a3aa45fd32ad9b645b68e431111f07` differs from authored a209/99b and exactly equals the unpublished local rebase `1a8a859a1658f2a730143be82dd3a8be972d7838/737`. The actual eight upstream paths and workflow/config/Reader/SameVoice/doc blob bindings are recorded in [audit.json](audit.json). This tree comparison does not imply binary equivalence or a completed current 737 Mac gate.

The actual 12-package ARM64 release nextest execution passed **7552 unique tests**, 0 FAIL/LEAK/TIMEOUT, in 161.816s across 70 selected binaries. All **21 SameVoice cases and 14 ordinary Reader controls passed exactly once**, including the changed Linux sentinel case within those 14. The run reported 53 aggregate skipped tests and 115 skipped binaries; individual skipped identities were not emitted. The Mac-only Reader case was not compiled; the isolated-root DAC case remained ignored.

All 34 post-summary PASS lines are configured final success-output reprints: 22 realtime and 12 privacy. Each matches its original identity/status/duration/ordinal; they are excluded from the 7552 execution count. Separate successful promotion 25, docs 4, process 13 and relay checks are not added to nextest totals. Whole-workspace Linux strict Clippy passed on the same preview.

Artifact metadata and the upload-log digest agree for artifact 12703856083 (`sha256:e7405db95e70a6b63e551596ec0e91700f2a2bcedf45decda7efa6806b4a8c03`). The exporter reported 47 current binaries, 2 selected examples and complete SHA256SUMS, 50 uploaded files. Only logs, metadata and source text were fetched; no archive or binary was downloaded or executed.

Historical [a209/99b Mac 21/242 coverage](../tests-restart-a209-host-20261027/README.md) retains its original source/owner scope. Native execution here covers the recorded 737+a209 preview; current 737 Mac qualification remains a separate gate.
