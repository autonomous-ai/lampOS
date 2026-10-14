# Schedule Voice qualification and integration records

Ready for lamp-voice: no

First settled-policy runtime checkpoint:
[`01192ba0fbfda04afb19e1db63aa86423c21fd6d`](https://github.com/autonomous-ai/lampOS/commit/01192ba0fbfda04afb19e1db63aa86423c21fd6d),
based on integration `6573ce6c1b594bf9c8cf7836bbd0e162c09e4cd1`.
The [checkpoint identity](checkpoint.json) records the frozen 697-path source
map, SHA-256 `e0c5985563535bf46d6af76d3bae71634ab3f1f03d8c713d55f202f95972ca7f`.
All those hashes match the revised commit. The source map's earlier Git HEAD
is retained as recorded before the amendment; it does not replace its exact
file hashes.

[The task verification folder](https://github.com/autonomous-ai/lampOS/tree/01192ba0fbfda04afb19e1db63aa86423c21fd6d/lampOS/docs/verification/schedule-voice-host-20261014/room-policy-6573)
retains the settled decision and completed focused checks: formatting, strict
all-target Clippy, 113 test executions / 0 failed / 2 ignored, and workspace
build on Third's Intel x86_64 Mac. Earlier failed complete gates keep their
original source identities and actual counts in the parent verification folder.

[Run 38626209078](https://github.com/autonomous-ai/lampOS/actions/runs/38626209078)
passed on `01192`: release workspace build/tests, **4,338 passed / 0 failed /
33 ignored** across 279 harnesses on GitHub's Ubuntu 22.04 ARM64 runner.
[Terminal evidence](native-01192/README.md) retains the raw log, result and
artifact checksums. The initial run list and job snapshot remain unchanged.
Four inherited Cargo output-filename warnings concern the two `replay_cost`
examples; this is not a warning-free workspace claim.

The complete affected-package host gate **failed: 1,602 passed / 1 failed /
8 ignored**, 62 harnesses, 1,732.493697235 seconds. The sole failure was the
unchanged motor authority assertion at `coordinator/motor/tests.rs:722` in
`motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`. That exact
built test passed once alone, **1 / 0 / 0**, in 1.519097596 seconds. This was
not the earlier line-441 startup timeout. The failed packet and peer were not
captured; no cause is inferred. [Both original runs](full-host/README.md)
remain separate. First's [#709 exception](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386)
allows the PR to list an untouched full-gate failure that passes alone with
its raw run and [#739](https://github.com/autonomous-ai/lampOS/issues/739); it
does not turn this complete gate into a pass.

The independent copy at `acb628b494df0c38842ea467d1289eec8d35d2ca` passed
workspace/helper builds and **287 tests / 0 failed / 2 ignored**, plus **72
default-off cases / 144 exact stdout comparisons** against a fresh `6573`
baseline. Both copies resolve all 34 local package manifests inside themselves.
[The standalone export](standalone-6573/README.md) preserves 612 original-path
mappings in 39 text files; [copy receipt](standalone-6573-copy-receipt.json)
and [export verification](standalone-6573-export-verification.json) record
the byte-for-byte retention. The only `01192` to `acb628` change clarifies
the disable runbook: daemon-reload, then restart Actions before Voice. All
697 frozen Rust/manifest hashes are identical. Native run
[38627179829](https://github.com/autonomous-ai/lampOS/actions/runs/38627179829)
for `acb628` is still pending at this checkpoint.

Integration advanced to `1337e7e1d8b88e102ec9fd12f544841569856f75`, adding
Days memory integration, timely Tasks news persistence and a Tasks-only
64 KiB reply bound. Task 31 rebased cleanly to
`f959aaa5e8b43b9ddea8d0031d0034272dcaa7af`; the
[rebase identity](integration-1337/rebase.json) preserves the old task paths
and verifies the resulting tree against the read-only merge preview.
[The incoming review](integration-1337/review.md) found no concrete behavior
conflict. `lamp-memory` now depends on Voice and joins the affected-package
gate. A new isolated regression will cover Days journalling with a kept-out
reminder followed by an ordinary exchange. New-source validation is pending;
none of the older runs is relabelled as a run on this rebase.

The [latest Queue snapshot](queue-1947.json) adds no Third task; the
[#739 snapshot](fixtures-1947.json) records Fourth's claim and First's
additional fixture report. [Initial retention](initial-retention.json) maps
the five original records and their hashes. This log branch contains no Task
31 runtime changes.

## Not shown

No passing complete host gate or qualified `1337`-based task head is claimed
at this checkpoint. No lamp, provider, physical audio, motion or power-cut
behavior was measured by Third. Command durations measure process launch
through exit/log closure on the named host, not lamp interaction latency.
First's runbook remains in
[schedule.md](https://github.com/autonomous-ai/lampOS/blob/01192ba0fbfda04afb19e1db63aa86423c21fd6d/lampOS/docs/schedule.md#task-31-lamp-qualification).
