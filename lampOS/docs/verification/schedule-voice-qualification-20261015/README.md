# Schedule Voice qualification and integration records

Ready for lamp-voice: no

Current task head:
[`d794cf2aca8fd5a7891af6363f2155e9fd3091fe`](https://github.com/autonomous-ai/lampOS/commit/d794cf2aca8fd5a7891af6363f2155e9fd3091fe),
based on integration `1337e7e1d8b88e102ec9fd12f544841569856f75`. All
700 frozen Rust/manifest/toolchain paths match the commit and clean working
tree ([identity](integration-1337/source-d794cf-identity.json)). Its
[focused evidence](https://github.com/autonomous-ai/lampOS/blob/d794cf2aca8fd5a7891af6363f2155e9fd3091fe/lampOS/docs/verification/schedule-voice-host-20261014/integration-1337/README.md)
records passing formatting, strict all-target Clippy, **136 test executions /
0 failed / 2 ignored**, and the workspace build. The new Days/reminder journal
exclusion test passed; it runs in both the reminder and remember filters.

The [fresh 1337 standalone copy](standalone-1337/README.md) passed the workspace
build and **435 test executions / 0 failed / 7 ignored**, including full Memory
package tests, plus **72 cases / 144 exact cross-copy stdout comparisons**.
Both copies resolve all 34 local manifests internally. The export retains
609 original-path mappings in 38 text files, with [export verification](standalone-1337-export-verification.json)
and [copy receipt](standalone-1337-copy-receipt.json). The [handoff audit](integration-1337/handoff-review.md)
found no incorrect claim or missing essential enable/disable step in its scope;
it is a source/document review, not execution evidence.

The complete five-package host gate started around 20:42 UTC on this frozen
source and is pending. Exact-head native ARM64 run
[38633124293](https://github.com/autonomous-ai/lampOS/actions/runs/38633124293)
is registered; [its initial observation](integration-1337/native-first-runs.json)
was pending behind the preceding checkpoint. [The qualification plan](integration-1337/qualification-plan.json)
keeps the source fixed while those runs complete. No terminal success for
either is claimed yet.

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
for `acb628` subsequently passed: **4,338 / 0 / 33**, 279 harnesses.
[Its terminal evidence](native-acb628/README.md) preserves the exact run and
all checksums; it is not a test on the newer integration.

Integration advanced to `1337e7e1d8b88e102ec9fd12f544841569856f75`, adding
Days memory integration, timely Tasks news persistence and a Tasks-only
64 KiB reply bound. Task 31 rebased cleanly to
`f959aaa5e8b43b9ddea8d0031d0034272dcaa7af`; the
[rebase identity](integration-1337/rebase.json) preserves the old task paths
and verifies the resulting tree against the read-only merge preview.
[The incoming review](integration-1337/review.md) found no concrete behavior
conflict. `lamp-memory` now depends on Voice and joins the affected-package
gate. The added isolated regression covers Days journalling with a kept-out
reminder followed by an ordinary exchange. The completed focused/copy stages
and pending complete/native stages above identify the new source; none of
the older runs is relabelled as a run on this rebase.

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
[schedule.md](https://github.com/autonomous-ai/lampOS/blob/d794cf2aca8fd5a7891af6363f2155e9fd3091fe/lampOS/docs/schedule.md#task-31-lamp-qualification).
