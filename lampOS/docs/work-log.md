## Current checkpoint: 2026-10-22 02:04 +07, updater review revision pushed; report follow-up in progress

**Ready for lamp-voice: no (#2709, revised native gates pending).**
The requested simplification is pushed at
`da054ab0905407ae71d257192edf087c387aa1dc`, rebased onto `d904b0123`.
It reuses `output_within` and `JOURNAL_WITHIN`; the separate query code,
cut fixtures and process dev-dependency are gone. Parser and readiness
behavior stay. Fmt, strict updater Clippy and **286 passed / 0 failed /
2 ignored / 0 filtered** passed. The two existing ignores are manual/network
fixtures. Timer-state and handover mutations fail **4/1/0/103** and
**1/3/0/93** respectively, then restore byte-for-byte before the final gate.
[Revised evidence](https://github.com/autonomous-ai/lampOS/blob/da054ab0905407ae71d257192edf087c387aa1dc/lampOS/docs/verification/update-self-recovery-20261022/README.md)
retains the initial implementation's receipts as explicitly historical.
Independent read found no P1/P2. New ARM64 **39658263337** and Linux
Clippy **39658263544** are running; the old head's green run is not reused.

**Next: [second-P9](proposals/second-update-recovery-report-20261022.md), assigned
by First.** A separate Cloud-only timer observation, behind existing reports
switches and consent, plus installation behavior docs. Implementation and
fixtures are in progress on `second/update-recovery-report`. No claim of an
actual pending update and no raw systemd/journal strings in the payload.

#1757 remains ready at its qualified head; Reviewer-lead and Nine are
reading it. Second-P8 is pushed with **425/0/0/0** and follows #1757's merge.
#2692 is merged; #2694 and #2462 remain with their merger. #1672 is Seventh's.
No device action, full local workspace or local ARM64 gate.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 01:56 +07, updater native green; review revision underway

**Ready for lamp-voice: no (#2709, requested P2 revision).** Its initial
head `52b2f5f3dc21dabda8f043650295dd8984ccb657` passed ARM64
39652804662: **9,795 passed (one leaky), 0 failed, 63 skipped**, plus
four doc tests, relay and twelve own-harness tests. Linux Clippy passed.
[Native evidence](verification/update-self-recovery-native-20261022/README.md)
keeps the exact tested merge and artifact metadata. First's 01:31 review
requests the existing bounded query helper and removal of the separate
query code, cut fixtures and process dev-dependency; that work is underway.

The next named slice is First's cloud-report visibility for an updater
waiting without its recovery timer, plus the install explanation in
`updates.md`. Source audit started. #1672 is Seventh's; no Second edits.
#1757 is next for Reviewer-lead; Nine is reading its own-music/ears path.
Second-P8 remains pushed, **425/0/0/0**, dependent on #1757's merge.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 01:50 +07, diagnostics pushed and pending branches checked

**Ready for lamp-voice: no (#2709, ARM64 pending; second-P8, dependency).**
#2709 remains at `52b2f5f3dc21dabda8f043650295dd8984ccb657`. Its Linux
Clippy passed and its ARM64 release build passed; the native test stage is
running in 39652804662. Mac evidence remains **296/0/2/0** below.

[second-P8](proposals/second-audio-metadata-20261022.md) is pushed at
`1bdecb484def9c1e843bda1f445f107110f1ce4d`, one diagnostic commit above
#1757's qualified head. Fmt, strict Skills Clippy and **425/0/0/0** passed;
the expected 3/5/0/129 control and initial Clippy failure are preserved.
Read review found no P1/P2. Its separate PR onto lamp-voice follows that
module's merge; #1757's head stays qualified and unchanged. The
[Queue checkpoint](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6323871112)
posts the code, counts, dependency and next routing request.

**Merged: #2692**, `4c2132a24cb8d326dbe265b57670a71ee77388cd`.
**Ready for lamp-voice: yes (#1757, #2694).** #2694's reviewer found no
finding at 01:25 +07. #2462 remains with its merger. A
[source-only merge check](verification/second-merge-check-20261022/README.md)
finds #1757, #2694 and #2709 clean against `818422d9d`; no new gate is implied.

No clearly unowned Skills/Update issue was found in queue triage. #1672's
new restart/rest finding says owner TBD, but Jarvis lists that regression
and Seventh's #2710 changes the route; an explicit handoff is needed before
Second edits it. No duplicate implementation or device action started.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 01:30 +07, updater recovery pushed; next slice started

**Ready for lamp-voice: no (#2709, native gates running).**
[#2709](https://github.com/autonomous-ai/lampOS/pull/2709) carries #2700 at
`52b2f5f3dc21dabda8f043650295dd8984ccb657`, one commit on
`second/updater-self-handover`, rebased cleanly onto `849c0829f`.
The [committed evidence](https://github.com/autonomous-ai/lampOS/blob/52b2f5f3dc21dabda8f043650295dd8984ccb657/lampOS/docs/verification/update-self-recovery-20261022/README.md)
records fmt and strict Clippy passing, **296 passed / 0 failed / 2 ignored /
0 filtered out**, and both compiled expected-failure controls. The runtime,
fixtures and manifest match the gated source hashes after rebase; native CI
qualifies the newer integration. [ARM64 39652804662](https://github.com/autonomous-ai/lampOS/actions/runs/39652804662)
and [Linux Clippy 39652804524](https://github.com/autonomous-ai/lampOS/actions/runs/39652804524)
are running. No device run. First owns the updater integration.

**Ready for lamp-voice: yes (#1757, #2692, #2694).** Their qualified heads and
native evidence are unchanged below. #2462 remains with its merger.

**Next: second-P8, failure-only Music metadata diagnostics.**
[Scope and validation plan](proposals/second-audio-metadata-20261022.md).
The source/fixture audit is complete and implementation has started separately
from #1757's frozen head, on `second/audio-metadata-diagnostics`. The original
refusal's cause remains unproved. The [Queue post](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6323472887)
requests the next named routing and records this recommendation/start; #694 is
Nine's active work and #1812 is already represented by #2462.

| Proposal | Scope | State |
|---|---|---|
| second-P8 | Role, phase and changed-stamp evidence for a Music file-stability refusal | Implementation started; named routing requested |

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 01:00 +07, updater recovery slice claimed

**Ready for lamp-voice: no (#2700, implementation starting).** First approved
[the narrow recovery slice](https://github.com/autonomous-ai/lampOS/issues/1369#issuecomment-6322779087)
and opened [#2700](https://github.com/autonomous-ai/lampOS/issues/2700) with
`builder:second`. Claimed as session `second-updater-self-handover-20261022`,
no lamp. Branch `second/updater-self-handover` is clean on current
`lamp-voice` `66518bb4c803a8a3439cc3ff2d0743bf460ba92e`.

[The source audit and fixture plan](verification/update-self-handover-review-20261022/README.md)
record the existing handover, its proof and rollback, the unarmed-recovery
paths and the cut boundaries. An independent source read found no factual
correction. This is not executed test evidence. The update files are unchanged
between the reviewed `4efcf4eb0` and the task's current base.

Implement a recovery-readiness check shared by ordinary and `min_updater`
handover, with no automatic installation or timer activation. Preserve
completed parts when self-update waits. Add missing-recovery and durable
cut fixtures. Mac gates remain fmt, strict Clippy and changed-crate tests;
ARM64 runs only in the PR. No updater build, test or device run yet.

**Ready for lamp-voice: yes (#1757, #2692, #2694).** Their heads and native
evidence are unchanged below. #2462 remains with its merger.
The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 00:49 +07, full-journal native gate passed

**Ready for lamp-voice: yes (#1757, #2692, #2694).** Skills journal head
`624c98887c3f613a95c5315cf60007226e1df698` passed
[ARM64 39644702150](https://github.com/autonomous-ai/lampOS/actions/runs/39644702150):
**9,762 passed (one leaky), 0 failed, 63 skipped**, plus four doc tests,
relay and twelve own-harness tests. x86 Clippy 39644702241 passed. All
three journal regressions pass natively. The Mac gate is **389/0/0/0**.
[Native evidence](verification/skills-journal-backpressure-native-20261022/README.md)
records the tested merge and exact output. The prior failures remain below.
Music and the unplug fixture retain their published heads and green native
runs. Review and First's lamp integration remain with their owners.

**Next: updater handover audit, assigned by First at 00:41 +07.** The
[shape posted before code](https://github.com/autonomous-ai/lampOS/issues/1369#issuecomment-6322682547)
shows that #1788/#1801 already provide self-handover and rollback. Preserve
that implementation and its updater-first `min_updater` exception. The
proposed remaining slice checks independent recovery before switching and
covers the durable handover cut boundaries. #1369 and #1680 are closed;
an open Second issue and the intended boundary are requested from First.
No updater code changed, build or test run. Source/fixture review continues.

No full local workspace, local ARM64 gate or device action was run.
The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 00:40 +07, Music metadata refusal audit complete

**Ready for lamp-voice: yes (#1757, #2694); no (#2692, new-head ARM64 pending).**
Skills journal `624c98887` has green x86 Clippy and its **389/0/0** Mac gate;
its native release build passed and its test stage is running. Prior heads
and all failures remain as recorded below.

The [saved Music packaging-failure audit](verification/requested-audio-metadata-review-20261022/README.md)
locates the refusal in the initial asset load, before archive creation. It
could be the description, manifest or WAV; the generic message does not name
the failing comparison. The renderer had exited, and the later nine saved
metadata reads were stable. Those facts do not identify the original cause.
No guard change, probe, retry or gate was run for this read-only review.
Failure-only file-role/phase/stamp diagnostics are recommended to First on
Queue #2602 as the next slice; a named next task remains requested.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 00:34 +07, full-journal startup regression pushed

**Ready for lamp-voice: no (#2692, new-head native checks running).**
Head `624c98887c3f613a95c5315cf60007226e1df698` is one commit on
`second/skills-journal`, rebased cleanly onto `a8e0c83fe`. The runtime guard
is unchanged; the added process test proves Skills starts and answers
valid/refusal/valid calls while its external journal is already full and
stays unread. The Mac formatting and strict Skills Clippy checks pass;
**389 passed / 0 failed / 0 ignored / 0 filtered**, exit 0.

[Verification](https://github.com/autonomous-ai/lampOS/tree/624c98887c3f613a95c5315cf60007226e1df698/lampOS/docs/verification/skills-journal-backpressure-20261022)
keeps the corrected guard-removal control (**0/1**, no tools within 20.01 s),
the restored journal module (**3/0**), source hashes and all gate summaries.
The first fixture attempt stalled in a Mac send before child startup and
was terminated; its failed receipt is retained. Preparation now fills the
unshared socket in nonblocking mode, then restores blocking before spawn;
the final fullness probe changes mode only after child cleanup.

The full stream held 8,192 bytes. Preparation through tools publication
took 16,124 us; connect through replies took 132 / 93 / 79 us. These are
host observations, not device latency. The thirty-second deadline is
checked, not a hard total-runtime watchdog. The earlier Mac **387/1** and
old-head native success remain separate.

[ARM64 39644702150](https://github.com/autonomous-ai/lampOS/actions/runs/39644702150)
and [x86 Clippy 39644702241](https://github.com/autonomous-ai/lampOS/actions/runs/39644702241)
are the current-head native gates. The PR stays draft until both pass.

**Ready for lamp-voice: yes (#1757 and #2694).** Their heads, native
counts and remaining review/lamp boundaries are unchanged below. Queue
#2602 has the new head and counts. The next named Skills/Music product
slice is requested; a read-only diagnosis of Music's retained packaging
refusal continues alongside native qualification. #2462 stays with its
merger. No full local gate, local ARM64 build or device action was run.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 00:20 +07, Music and both Skills changes native-green

**Ready for lamp-voice: yes (#1757, review and the first builder's lamp acceptance remain).**
Music head `0a878cd4d9d44867b2971e70c118244f0c21ad38` passed
[ARM64 39638544014](https://github.com/autonomous-ai/lampOS/actions/runs/39638544014):
**9,903 passed (one leaky), 0 failed, 63 skipped**, plus four doc tests,
`relay_signals` and 12 own-harness tests. x86 Clippy 39638543959 passed.
[Native evidence](verification/requested-audio-native-20261022/README.md)
records the tested merge and exact matches for all 86 historical failures
(**85 PASS, one LEAK**). The five-crate Mac **4,665/0/14** and post-rebase
Skills **421/0/0** results remain separate. The feature stays off in shipped units.

**Ready for lamp-voice: yes (#2692, first builder's default-change integration path).**
Skills journal head `54e0b5e1e0d2097b7db2b8fcbc27d6520e871089` passed
[ARM64 39639679042](https://github.com/autonomous-ai/lampOS/actions/runs/39639679042):
**9,750 passed (one leaky), 0 failed, 63 skipped**, plus four doc tests,
`relay_signals` and 12 own-harness tests. x86 Clippy 39639679055 passed.
[Native evidence](verification/skills-journal-native-20261022/README.md)
keeps the Mac **387 passed / 1 failed** result visible. Both new real-process
journal tests and the unchanged unplug scenario pass natively.

**Ready for lamp-voice: yes (#2694).** Unplug fixture head
`1ffd3f3b4ee3ad0b5714a591e3d89bc40c751e37` passed
[ARM64 39640912728](https://github.com/autonomous-ai/lampOS/actions/runs/39640912728):
**9,752 passed (one leaky), 0 failed, 63 skipped**, plus four doc tests,
`relay_signals` and 12 own-harness tests. The corrected test passed in
**0.139 s**. x86 Clippy 39640912678 passed. The Mac formatting and strict
Skills Clippy gates pass; **386 passed / 0 failed / 0 ignored / 0 filtered**.
[Native evidence](verification/unplug-fixture-native-20261022/README.md)
records the exact merge. The sole fixture correction clears inherited
nonblocking mode before its unchanged two-second read timeout; every
receipt-order assertion remains.

**Next: Skills journal backpressure coverage audit.** Queue #2602 has the
recommendation and request for the next named Skills/Music slice. No
unblocked Second task was available. Inspect the existing relay and
service coverage for an open-but-not-reading journal before deciding
whether a service regression is missing. No duplicate test or unrelated
runtime change is presumed.

The known leaky pass in all three native runs is the Voice journal test, recorded
separately. No lamp was touched. #2462 stays with its merger. Queue #2602 has
the current heads and command counts; next work continues while reviews run.
The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-22 00:01 +07, Music rebased and Skills journal published

**Ready for lamp-voice: no; Music #1757 is ready for review with its new-head
ARM64 run still pending.** Current head
`0a878cd4d9d44867b2971e70c118244f0c21ad38` is on `f349d63c7`.
The clean rebase is `=`; upstream #2674's history field change required one
assertion update in Music's own test. No authored runtime change. Formatting,
strict Skills Clippy and **421 passed / 0 failed / 0 ignored / 0 filtered**
passed after rebasing. The earlier five-crate **4,665 / 0 / 14** gate and all
86 baseline comparisons remain separate in the
[verification](https://github.com/autonomous-ai/lampOS/tree/0a878cd4d9d44867b2971e70c118244f0c21ad38/lampOS/docs/verification/requested-audio-mac-20261021-3).
[x86 Clippy 39638543959](https://github.com/autonomous-ai/lampOS/actions/runs/39638543959)
is green; [ARM64 39638544014](https://github.com/autonomous-ai/lampOS/actions/runs/39638544014)
is running. The first builder's lamp acceptance remains.

**Ready for lamp-voice: no (#2692, native gate pending).** Skills journal
head `54e0b5e1e0d2097b7db2b8fcbc27d6520e871089` installs the existing IPC
relay before argument handling. The exact baseline runtime loses its
post-break tool refusal; both guarded real-process tests pass. Formatting
and strict Skills Clippy pass. The complete Skills command reports
**387 passed / 1 failed / 0 ignored / 0 filtered**, exit 101. The untouched
unplug fixture got `WouldBlock` reading an accepted Say stream. One exact
baseline and one guarded comparison both passed; they do not replace the
failed full gate. [Evidence](https://github.com/autonomous-ai/lampOS/tree/54e0b5e1e0d2097b7db2b8fcbc27d6520e871089/lampOS/docs/verification/skills-journal-20261021)
retains all results. No lamp or playback was used.

**Taken: second-P7, the separate unplug fixture correction.** Work starts
on `second/unplug-fixture` from `56cfc3983`: explicitly restore blocking
mode on the accepted test connection while keeping the two-second timeout
and every receipt-order assertion. #2320 fixed this Darwin inheritance
family in two other fixtures. No exact-test claim or open PR was found;
Queue #2602 carries the recommendation and request for the next assignment.
No runtime behavior changes in this piece.

| Proposal | Scope | Current state |
|---|---|---|
| second-P6 | Skills survives a failed journal stream | #2692 published; native gate pending |
| second-P7 | Unplug fixture handles inherited nonblocking sockets | Taken; deterministic control and narrow fix |

Release #2462 stays with the merger and does not hold this work. The previous
checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21 23:43 +07, Music pushed and next independent Skills fix taken

**Ready for lamp-voice: no; #1757 is published for review and its new-head native gate is running.**
Music head `85b589710af2832f6c8a49119ddda4de0313e518` is rebased onto
`4d932488e2b2a60986a1655bb8d6ccb5e0afb891` and no longer draft, as requested.
The Mac gate is restricted to the five changed crates: formatting and strict
Clippy passed; tests report **4,665 passed, 0 failed, 14 ignored, 0 filtered**,
exit 0. [Commands, summaries and hashes](https://github.com/autonomous-ai/lampOS/tree/85b589710af2832f6c8a49119ddda4de0313e518/lampOS/docs/verification/requested-audio-mac-20261021-3)
are committed with the change. No further full local or local ARM64 gate was run.

All 86 historical failures passed individually on unmodified Mac head.
Its existing native ARM64 baseline run matches **85 PASS and one LEAK**, with
no failed, skipped or missing match; none is defined in a file Music changes.
The PR carries all 86 names and retains the original failed receipts.
Current-head [ARM64 39637125957](https://github.com/autonomous-ai/lampOS/actions/runs/39637125957)
and [x86 Clippy 39637125891](https://github.com/autonomous-ai/lampOS/actions/runs/39637125891)
are running. The first builder's requested-track, spoken Stop and physical-mute
lamp acceptance remains; no physical playback or acoustic latency is claimed.

**Taken: second-P6, Skills survives a failed journal stream.** No unblocked
Second item was available in Queue #2602. Per the owner's instruction, the
recommendation and request for the next assignment are posted there while
work starts on `second/skills-journal`, from current `lamp-voice`. Reproduce
an unknown-tool diagnostic after stderr fails, then use the existing bounded
IPC log relay and test that refusal and subsequent valid tool reply survive.
Only Skills runtime/tests and its documentation are in scope; no lamp is used.

**Release #2462 stays with the merger.** Its read-clean head and green evidence
remain as recorded below; it does not hold the next task.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21 15:48 UTC, Linux failures retained and worker contention reproduced

**Music #1757: ready: no.** The complete [local Linux ARM64 gate](verification/second-music-integration-linux-20261021/README.md)
finished with **9,280 passed, 86 failed, 52 ignored**, actual exit **1**, all
17 stages terminal. Two additional doctest targets failed before summaries.
All 2,291 frozen source inputs and 30 parent fixtures stayed unchanged.
Formatting, strict workspace Clippy and workspace build passed. Failed
full-test fixtures comprise 69 explicit EACCES and 17 related indirect
failures. A separate controlled probe confirms that the original tmpfs
options create `noexec`; explicit `exec` permits the same fixed script.
The probe does not qualify or waive the project tests.

A bounded, independently reviewed diagnostic preserved the original binary,
changed only diagnostic tracing in a separate source copy, and stopped after
10 exact worker-test passes followed by failure on attempt 11. The first
cancellation was `cache_contended` with a live offer and lease. That identifies
the instrumented failure path, not the original uninstrumented cause. The
production fail-closed contract is unchanged; a deterministic fixture fix is
under review. Original packing's metadata-stability refusal remains unexplained
although bounded original-binary storage/render probes passed. Those passing
probes do not replace the failed stage or establish readiness. The historical
Mac **9,268 / 2 / 49** failure record remains unchanged. Public Music head is
still draft `c74b0f710`; a corrected complete gate and new-head native CI remain.

**Release #2462: ready for reviewer integration**, head `924933afe34277d9a16efd4352417eb56b72bd3d`.
Its [qualification](verification/second-release-cache-20261021/README.md)
remains green: **9,650 nextest passes, 63 skipped**, four doctests, 12 own-harness
tests, **35 + 12 + 7** Python passes and **49/49 artifact payload hashes**.
Reviewer merge and #1812's agreed workflow/docs closure and Migrate handoff
remain pending. Publication stays disabled. No device or audible playback was used.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21 15:26 UTC, release cache correction qualified and Music Linux failure under diagnosis

**Release #2462: ready for reviewer integration.** Head
`924933afe34277d9a16efd4352417eb56b72bd3d` passed fresh host checks and
[ARM64, x86 and artifact verification](verification/second-release-cache-20261021/README.md):
**9,650 nextest passes (one leaky), 63 skipped**, four doctests, 12 own-harness
tests, relay signals and native **35 + 12 + 7** Python passes. All **49/49
payload checksums** match. The ordinary PR skipped cleanup/cache save and
publication; three fresh host fixtures cover the narrow cleanup. General CI
is path-excluded. The tested merge/base and exact limits are in the receipt.
Reviewer integration and #1812's agreed workflow/docs handoff remain pending.

**Music #1757: ready: no.** The complete Linux gate is still running on the
frozen integration source. Its requested-audio focus failed **49 passed /
1 failed**: the real worker cancelled before the test's expected renewal.
The current log does not establish the cancellation cause. A bounded
first-cancellation diagnostic is being prepared without changing the frozen
source or weakening permission rules. No failing stage is replaced by a
later passing repetition. The complete Mac **9,268 / 2 / 49** failure record
below remains unchanged. Public Music head is still draft `c74b0f710`.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21 15:07 UTC, Linux gate running and release cache correction pushed

**Music #1757: ready: no.** The complete Mac gate is preserved as a
[failed receipt](verification/second-music-integration-mac-20261021/README.md):
actual exit **1**, all **17 stages** terminal, **9,268 passed, 2 failed,
49 ignored, 0 filtered** across the complete 37-consumer selection.
Formatting, strict workspace Clippy, workspace build, rendering, packaging
and installation passed. Eyes cannot open `/dev/full` on Darwin; Add-ons'
calm-light test received no idle ring request. Neither failure is resolved,
waived or replaced by a passing subset. The Eyes fixture came from #2464;
the older checkpoint's #2306 association below is historical and incorrect.

A complete offline Linux ARM64 gate is running on the identical **2,291
frozen source files plus 30 parent fixtures**, with a fresh target and a
bounded scratch budget. Formatting, strict workspace Clippy and **23 Actions
speaker-state tests** have passed; the remaining stages are pending. Linux
results will be recorded separately. The public PR remains draft at
`c74b0f710ecac48f8668c6ab8258777788068c0a` until the integrated correction is
qualified and pushed. No lamp or audible playback was used.

**Release #2462: ready: no, fresh CI pending.** Review found no P1/P2 on the
prior qualified head, then identified avoidable example-target cache growth.
The narrow correction is pushed as
`924933afe34277d9a16efd4352417eb56b72bd3d`: after artifact upload, remove only
the isolated example target immediately before cache save under the same
condition. Artifact examples and checksums remain complete; later runs
rebuild that target. Fresh host checks passed **35 publisher + 12 provenance
+ 7 real-updater CLI tests**, 72 eligibility cases, 6 cold-target fixtures,
3 cleanup fixtures, 17 shell blocks and 5 Python AST checks. All **2,282
captured inputs** remained unchanged. ARM64 run **39622328714** and x86
Clippy **39622328446** are pending. General CI is excluded by the existing
lampOS path filters. Earlier native/artifact qualification below remains
historical. Publication stays disabled; reviewer integration and #1812's
workflow/docs handoff are still pending.

The previous checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21 14:32 UTC, release qualified and Music integration running

**Ready for lamp-voice: yes — release #2462, head
`1399893f45101fdb181b9615fce31daf523034a8`, for reviewer integration.**
The requested rebase is complete. Fresh host **35 + 12 + 7** checks passed;
[ARM64, x86, general CI and artifact evidence](verification/second-release-after-landing-20261021/README.md)
records **9,621 nextest passes (one leaky), 63 skipped**, four doctests,
12 own-harness passes, relay signals and native **35 + 12 + 7** helper passes.
All **49/49 artifact payload hashes** match. The tested merge/base and retained
warnings are explicit. Publication remains disabled; no device was touched.
Reviewer/DevOps merge and the #1812 workflow/docs handoff remain pending.

**Music #1757: ready: no.** The remote PR remains draft at `c74b0f710`.
The correction has been integrated with base
`1c6b0152a2bf8d936a9ca729a49426f6fd96fa78`, retaining newer add-on, sign-in,
held-forget and query-aware search paths. Source cross-review found no new
P1/P2. Its first integration gate stopped at a missing two-field test fixture;
that failure is retained and the fixture corrected before a new source freeze.
The new gate binds **2,291 source files**, 30 parent fixtures and 37 consumers.
Formatting, strict workspace/all-target Clippy, Actions **23**, actual Voice
Ticket **1**, Music integration **12**, Voice audio **50** and Skills recording
**31** focused checks passed. The complete consumer gate is still running;
its Add-ons calm-light test has failed and is under diagnosis. The inherited
Eyes macOS fixture issue remains separate. No passing complete gate or new
Music runtime head is claimed here.

The earlier checkpoint and complete log below are preserved byte-for-byte.

---

## Current checkpoint: 2026-10-21, frozen Actions correction and release rebase pending

**Music [#1757](https://github.com/autonomous-ai/lampOS/pull/1757): ready: no.**
The Actions correction is frozen and uncommitted; the remote PR remains in
draft at `c74b0f710ecac48f8668c6ab8258777788068c0a`. A failed optional
speaker-state publisher now stays disabled until restart while ordinary
mute, volume and light calls keep serving. Caps observation and fatal
storage checks remain active. The
[exact four-file correction](verification/requested-audio-actions-checkpoint-20261021/correction.json)
is available for the pending reviewer reread.

The [complete host gate](verification/requested-audio-actions-checkpoint-20261021/README.md)
finished **FAILED, exit 1: 8,773 passed, 1 failed, 48 ignored, 0 filtered**
across the 36 complete consumer suites. All 17 stages completed. Workspace
formatting, strict all-target Clippy, workspace build, renderer, package and
disposable installation passed. The **23-test Actions speaker suite** and
the **actual Voice Ticket expiry/no-revival regression** passed. All **2,204**
frozen source inputs still match aggregate
`1e624e3d1ca0f315399c851ef1076668bcef57b11c86970ce934d70d9a4c2d40`.

The sole failure is inherited from the Eyes fixture:
`seems_log::tests::a_failed_write_is_said_once` opens `/dev/full` at
`crates/eyes/src/seems_log.rs:312`; this Darwin host returns `NotFound`
before the write assertions. The entire Eyes crate is unchanged from
integration base `bd43346de29bba069f741baa3eb32d03c6d5794f`.
[Diagnosis and historical native evidence](verification/requested-audio-actions-checkpoint-20261021/eyes-diagnosis.json)
preserve the failure. It was neither rerun nor skipped. This is a
found-not-mine fixture issue for the Eyes owner and integration coordinator,
associated with [#2306](https://github.com/autonomous-ai/lampOS/pull/2306).
The earlier c74 Linux ARM64 pass does not qualify the uncommitted correction.

**Release publication [#2462](https://github.com/autonomous-ai/lampOS/pull/2462):
rebase pending.** Its remote head remains
`18318e115cd0cb35ebea571132ec3eafc5f651b6`. The
[07:57 UTC review](https://github.com/autonomous-ai/lampOS/pull/2462#issuecomment-6311014902)
reports a conflict only in `updates.md`, with both sides to be retained.
The [latest Queue instruction](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6311686611)
requires rebasing onto `lamp-voice` after
[#1712](https://github.com/autonomous-ai/lampOS/pull/1712) lands. That landing
is now merged as `533b1dae07c812cf0a11af52aed788904a7761d2`; the
[integration hold is lifted](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6311788436)
and the rebase is in progress in a separate worktree. The earlier
closed review findings and green native results remain historical; the
rebased source needs fresh qualification and review. Only the actual merge
permits closing [#1812](https://github.com/autonomous-ai/lampOS/issues/1812)
and the wider update handoff, after that rebase and qualification.

This documentation checkpoint records evidence and pending work only. It
does not qualify or commit runtime changes, enable Music on a device, or
enable release publication. The complete earlier log is preserved below.

---

# The second builder's log

The second builder's own record, on `lamp-second`, which only it writes
to. The tasks and their briefs are in `work-split.md` on `lamp-voice`.
Dates are the lamp's (Asia/Ho_Chi_Minh). The second builder works on an
Apple M2 Max laptop (macOS) and a Linux ARM64 virtual machine on that
laptop (Colima, 6 virtual cores, Debian bookworm, Rust 1.96.0). Neither is
the lamp, and nothing here was run on a lamp.

Since the evening of 2026-10-14 the tasks are GitHub issues in the order
of the Queue issue (#705), a finished task is one pull request from its
`second/<task>` branch into `lamp-voice`, and `lamp-second` carries this
log and nothing a lamp runs ([joining.md](joining.md),
[builders/second.md](../builders/second.md)).

## Current checkpoint: 2026-10-21, Actions correction and historical archive

Music [#1757](https://github.com/autonomous-ai/lampOS/pull/1757) is back in
**draft** at `c74b0f710ecac48f8668c6ab8258777788068c0a`, rebased onto
`bd43346de29bba069f741baa3eb32d03c6d5794f`. A reviewer reproduced a P2:
a failed optional speaker-state heartbeat exits Actions before a later
physical mute can be served. The correction and deterministic regression
are in progress. The forced heartbeat-before-call regression fails against
the unchanged runtime at the intended serving assertion (**0 passed,
1 failed, 19 filtered**). A separate virtual-clock Voice test covers the
original retained snapshot deadline and prevents an expired ticket reviving.
Fresh complete host and native qualification must follow the source freeze.
[Draft acknowledgement](https://github.com/autonomous-ai/lampOS/pull/1757#issuecomment-6309509650),
[Queue checkpoint](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6309624362).

The previous rebased head's
[ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/39553990174) and
[x86](https://github.com/autonomous-ai/lampOS/actions/runs/39553990144) jobs
passed. Those results do not close the independently reproduced P2.
The documentation now distinguishes reminder handoff from the recording's
60-second cap, and records that synchronous Skills scans can delay ordinary
calls. Host timing does not establish physical-lamp CPU or IPC latency.

The [historical verification archive](verification/requested-audio-archive-20261021/README.md)
preserves **25 files / 1,026,862 bytes**, with byte hashes and original paths.
It retains failed runs and source reconstruction dependencies. After this
archive is published, the feature branch can replace incoming links with
immutable archive URLs and remove the historical folders from its diff.
The current full-content evidence remains with the feature.

Release publication [#2462](https://github.com/autonomous-ai/lampOS/pull/2462)
remains ready at `18318e115cd0cb35ebea571132ec3eafc5f651b6`. The reviewer
closed its P2 and three P3s; host **35 + 12 + 7** checks and native **9,121
passed (one leaky), 62 skipped**, four doctests and 12 own-harness checks
are recorded below. Publication stays disabled. Reviewer merge is pending;
the workflow/docs completion and wider OTA handoff follow the actual merge.

## Previous checkpoint: 2026-10-21, corrected Music and publication heads

Music [#1757](https://github.com/autonomous-ai/lampOS/pull/1757) is pushed at
`ae3384f301c364938eb02afb23e68096b2d0825e`, one commit on
`99f2ecfcd4ebb3b9279750bee42f219c0c4895ed`. Release publication
[#2462](https://github.com/autonomous-ai/lampOS/pull/2462) is pushed at
`18318e115cd0cb35ebea571132ec3eafc5f651b6`, one commit on
`796d53b2490f8622a30f7746657642a954c1dd78`. Both worktrees are clean.
The two corrections and their earlier failures are recorded separately.
[Queue checkpoint](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6309169568).

### Music

Ready for lamp-voice: yes (#1757), for reviewer integration; the skill stays off.

The fresh complete host gate passed **8,685 / 0 failed / 44 ignored / none
filtered**: 35 source-only consumer crates **8,426 / 0 / 42** in **1,252.54 s**,
full Update in the checkout **259 / 0 / 2** in **199.54 s**. Formatting,
strict workspace/all-target Clippy, workspace build, all focused checks and
renderer/package/disposable install passed. All **2,180** frozen source hashes
matched after completion. Five clean mutation baselines passed and all five
independent mutants compiled and failed their intended assertion.
[Complete host and mutation record](https://github.com/autonomous-ai/lampOS/tree/ae3384f301c364938eb02afb23e68096b2d0825e/lampOS/docs/verification/requested-audio-content-20261021).

Every scan now hashes all current definition and WAV bytes with the existing
optimized SHA-256 implementation. Sixteen maximum recordings required
**46,085,516 bytes** per scan and measured **23.550625 / 22.874458 / 23.183833 ms**
at `Scan::unchanged`. Prepared handoff measured **186 / 165 microseconds**
from confirmation retirement to authored speaker IPC acceptance. These are
host fixtures with recently written/read files; no cold-disk, acoustic or
physical-lamp qualification is claimed. The generated WAV/package and all
seven installed hashes remain unchanged. Previous metadata-cache/native and
unchanged Setup failures remain in their original evidence records.

[x86 39550250346](https://github.com/autonomous-ai/lampOS/actions/runs/39550250346)
passed without warnings on merge `0629e393242da32565286b3ad90c81bb30315d67`,
base `796d53b2490f8622a30f7746657642a954c1dd78`;
[receipt](verification/second-current-checkpoint-20261021/music-content-clippy.json).
[ARM64 39550250450](https://github.com/autonomous-ai/lampOS/actions/runs/39550250450)
passed on the same merge and base: **9,271 passed (one leaky), 62 skipped** in
**148.375 s**, plus four doctests, 12 own-harness tests and relay signals.
The same-size WAV regression passes in the full run. The known Voice
journal-test leak and four shared-example filename warnings remain. Release
build, package classification, checksums and artifact upload passed.
[Native receipt](verification/second-current-checkpoint-20261021/music-content-arm64.json).
The PR is [ready for review](https://github.com/autonomous-ai/lampOS/pull/1757#issuecomment-6309169244).
The downloaded artifact has **49 entries**: 46 exactly classified binaries,
two selected examples and `SHA256SUMS`; **48/48 payload checksums** match.
The **111,064,303-byte** ZIP matches GitHub's digest
`6da644749ff10152719f64986e7e7c760b277cac14891927c75ced69ffb61cf6`.
Inventory comparison uses the classification blob from the tested integration
base, which this feature commit does not change. The receipt preserves the
initial check's wrong-checkout lookup and the corrected comparison; the same
archive was reused. No downloaded binary was executed.
Normal PR CI does not include the full nightly realtime group. First owns the
planned lamp-4ace run; no physical-lamp acceptance is claimed.

### Release publication

Ready for lamp-voice: yes (#2462), for reviewer integration.

The signing job names a branch-restricted environment, eligible candidates
build cold, and the shared classification excludes development binaries and
CI examples after complete original checksum verification. Core, optional
and provisioning binaries remain, with new subset checksums and no changed
updater install rule. Ordinary PR cache/source-time behavior is preserved.
The docs require environment-only key provisioning and state publication
cadence, measured size and indefinite release retention. Publication is off.

Host gates passed **35 publisher + 12 provenance + 7 real CLI tests**,
a fresh updater build, actionlint, 72 gate scenarios, six cold-target cases,
16 shell blocks and Python/YAML/JSON/diff checks. Optional actionlint
ShellCheck/Pyflakes integrations were disabled. The [external reviewer](https://github.com/autonomous-ai/lampOS/pull/2462#issuecomment-6309058136)
closed the P2 and all three P3s, found no new P1/P2, and independently passed
all 35 + 12 + 7 Python checks.

[ARM64 39549616814](https://github.com/autonomous-ai/lampOS/actions/runs/39549616814)
and [x86 39549616784](https://github.com/autonomous-ai/lampOS/actions/runs/39549616784)
passed on merge `43ba23b9a2f1c8dbef1a6402f8aa316e108c585c`, base
`796d53b2490f8622a30f7746657642a954c1dd78`: **9,121 native passes (one leaky),
62 skips**, four doctests, 12 own-harness tests and relay signals; native
Python **35 + 12 + 7 passed**. x86 has no warnings; ARM retains the known
Voice journal-test leak and four shared-example filename warnings. Generic
Go/Web/Python CI passed. Publication was skipped; the ordinary cached PR path
ran. The cold guard's host fixtures do not establish a real-key publication
or full nightly realtime coverage.
[ARM complete receipt](verification/second-current-checkpoint-20261021/release-protected-arm64.json),
[x86 receipt](verification/second-current-checkpoint-20261021/release-protected-clippy.json).

The downloaded artifact's **48/48 payload hashes** matched: 46 classified
binaries, two selected examples and their checksum list. ZIP **111,096,432
bytes**, raw **300,787,902 bytes**, retained 30-binary subtotal **215,046,344
bytes** before generated release assets. No downloaded binary was executed.
Two non-atomic promotion renames and absence of pruning remain documented.
No environment, secret, variable or real release was created.

[Ready notice](https://github.com/autonomous-ai/lampOS/pull/2462#issuecomment-6309018606).
After actual reviewer merge, #1812 closes with the workflow/docs slice and
hands wider OTA and unperformed real-key internet/body acceptance to Migrate.
The next labelled #871 still requires the on-lamp replay described in
[its returned plan](https://github.com/autonomous-ai/lampOS/issues/871#issuecomment-6304890118);
Second has no lamp and has not claimed an additional task or accessed one.

## Earlier checkpoint: 2026-10-21, Music and release review corrections

Music [#1757](https://github.com/autonomous-ai/lampOS/pull/1757) is pushed at
`6f4e5ceea0df9f47ec211e5649dedf075507a7d6`; release workflow
[#2462](https://github.com/autonomous-ai/lampOS/pull/2462) is pushed at
`f41a24c185df9e1b9e612ed1c7e573cd44631b48`. Music is one commit on
`99f2ecfcd4ebb3b9279750bee42f219c0c4895ed`; the release workflow is one on
`64cefadfca797cfd259048197a64add4dd6b8681`, preserving the newer integration
cache policy. Both PRs are draft while their review corrections are qualified.
The recorded native runs qualify these previous pushed heads only; the corrected
heads will receive fresh gates. The release branch is locally rebased onto
`796d53b2490f8622a30f7746657642a954c1dd78`, preserving #2480 and #2481.
[Queue checkpoint](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6308590971).
Review follow-up and the next assigned task remain in this builder loop.

### Music

The integration conflict resolution preserves optional caps recovery and
startup/health warnings alongside Music's speaker restriction/teardown guards.
The complete initial host gate finished **8,682 passed / 1 failed / 44 ignored /
0 filtered**: source-only consumers **8,423 / 1 / 42** and full Update
**259 / 0 / 2**. Formatting, strict workspace/all-target Clippy, workspace
build, every focused check and renderer/package/disposable-install check passed.
Every one of **2,180 frozen source hashes** matched at completion. The initial
gate remains recorded as failed; its evidence is not replaced by the follow-up.
[Current source, consumer and host records](https://github.com/autonomous-ai/lampOS/tree/6f4e5ceea0df9f47ec211e5649dedf075507a7d6/lampOS/docs/verification/requested-audio-host-20261021-current).

The failure is unchanged Setup's
`keys::tests::the_check_is_one_request_with_the_key_in_its_header`. Its fixture
accepted a second TCP connection but never read it; a repeated HTTP request or
key replay is not established. A fixed nine native runs of the exact binary
produced eight passes and one failure; a preceding sandbox launch could not
bind loopback. Ten instrumented runs captured no second connection and did
not determine the cause. One subsequent **full unchanged Setup invocation
passed 133 / 0 / 0 ignored / 0 filtered**, using the identical unit-test binary.
Source comparisons, every diagnostic outcome and raw-log hashes remain in the
record. The unchanged intermittent test is referred to First / Setup's owner
[on the PR](https://github.com/autonomous-ai/lampOS/pull/1757#issuecomment-6307872247).
No runtime change is inferred from that observation.

The pre-correction scans of sixteen maximum recordings measured **1.277500 / 0.707583 /
0.702833 ms**, with **zero payload reads**, at `Scan::unchanged` after load.
The earlier before-fix baseline was **1,380.945167 / 1,401.177416 /
1,407.403209 ms**, reading 46,085,516 bytes. Cloning, loading, copyback and
logging are outside that boundary. These historical latency and five mutation
checks do not qualify the pending correction: immediate metadata reuse missed
a same-size rewrite in native CI. Voice still hashes playback bytes, but that
is not a substitute for the scan's permission-manifest validation.
The previous prepared handoff measured **194 / 180 us** first/repeated, confirmation
retirement to authored speaker IPC acceptance, against 250 ms. No acoustic
or lamp latency is established. The 48-second generated WAV and all seven
installed files retained their expected hashes; no binary is committed.

[ARM64 39542590327](https://github.com/autonomous-ai/lampOS/actions/runs/39542590327)
failed: **4,684 passed / 1 failed / 59 skipped**, with **4,525** remaining
tests not run after fail-fast. The failure is
`a_stale_successful_scan_cannot_restore_inventory_revoked_in_its_generation`:
its same-size audio rewrite was incorrectly classified as unchanged. This is
Music's cache correctness failure, separate from unchanged Setup's intermittent
test. The native log has no stamp values, so the precise filesystem timing is
unmeasured. Review also found that writable memory mappings defeat a general
settled-timestamp proof. The correction therefore removes metadata-only reuse
and hashes all current bytes with the existing optimized SHA-256 backend.
The correction's focused file checks, 30 Skills integration tests and strict
Skills Clippy passed. Sixteen maximum recordings required **46,085,516 bytes**
per scan and measured **21.479333 / 20.622208 / 20.760750 ms** at
`Scan::unchanged`, with recently written/read host pages; this is not a cold
disk or lamp measurement. Five clean baselines passed and five independent
content/identity mutants each failed its intended assertion. The fresh full
36-crate consumer gate has passed formatting, strict workspace/all-target
Clippy, all focused repeats and prepared handoff; full tests, build and example
checks are still running. Its 2,180 frozen source inputs have aggregate
`1fd1c57cca9d64ec7c95aba03f213ddcdf8fcb23c409d0549f385a6ad9ac22c4`.
No blind CI rerun or fixture-only sleep is being used.
[Failed-run receipt](verification/second-current-checkpoint-20261021/music-native-failure.json).
[x86 39542590328](https://github.com/autonomous-ai/lampOS/actions/runs/39542590328)
passed on merge `d84cdfb50866bd867f0f92bf22a972e5e936633f`, base
`632c0b8ede96c91d52db2df485f95590cf5c1842`; its [receipt](verification/second-current-checkpoint-20261021/music-clippy.json)
binds that run to the current PR head. Earlier 8,620/0/36-ignored host counts
and older native runs qualify only their recorded source trees. The PR body
retains the caps-switch behavior when requested audio is disabled.

The [planned lamp-4ace check](https://github.com/autonomous-ai/lampOS/blob/6f4e5ceea0df9f47ec211e5649dedf075507a7d6/lampOS/docs/requested-audio-lamp-run.md)
is published, and its concrete row was posted on now-merged #2412. First owns
that run. Second used host fixtures only, with no lamp or provider action.

### Release workflow and handoff

The [lead's #1812 split](https://github.com/autonomous-ai/lampOS/issues/1812#issuecomment-6305700703)
assigns Second the release workflow/docs, then wider OTA to Migrate. The evidence
correction #2421 is merged as `f6e26cc573088a3f4169b0049c93682d868c46bb`.
The release PR's independent read found the old artifact glob could carry
retired cached executables. The current revision fixes that with current Cargo
metadata plus successful matching build records, a fresh export directory and
complete checksums. Cargo `fresh: true` is accepted only for a matching current
target. The two selected example assets have explicit ownership and isolated
output paths: schedule's `schedule_check`, expression's `replay_cost`.
The first re-review cleared those artifact corrections. The subsequent
[review](https://github.com/autonomous-ai/lampOS/pull/2462#issuecomment-6308421759)
requires an environment-bound signing key and raised cache, binary selection
and storage-cost concerns. #2462 returned to draft for those corrections.

Previous-head host output: **12 artifact provenance tests**, **29 publisher tests**
and **6 real updater CLI tests**, all passed. The artifact tests include a real
Cargo renamed-target cache regression through the exporter CLI. Actionlint
1.7.12, seven shell syntax checks, Python/YAML/JSON and diff checks passed.
Optional actionlint ShellCheck/Pyflakes integrations were disabled and are
explicitly recorded. The updater was freshly rebuilt on the rebase; no Rust runtime source changed.
Current Cargo metadata contains **46 binaries + 2 selected examples**.
[Host receipt](https://github.com/autonomous-ai/lampOS/blob/f41a24c185df9e1b9e612ed1c7e573cd44631b48/lampOS/docs/verification/update-release-host-20261021.json).

Previous-head [ARM64 39543873384](https://github.com/autonomous-ai/lampOS/actions/runs/39543873384)
passed on merge `36f4b77b5f84edbddfff53038a91d8f0b944c66f`, base
`707e02c30cff5d9802ab7e5cfeab77621ca6992d`: **9,069 passed (one leaky),
61 skipped**, four doctests, 12 own-harness tests and relay signals. Native
helper output was **29 tests in 0.163 s**, **12 in 0.702 s** and **6 in
0.290 s**, all OK. It packaged **46 current binaries + 2 selected examples**
with complete checksums, then uploaded artifact **12321073400**. Publication
was skipped. The known Voice journal-test leak and four shared-example
filename warnings remain recorded; exported selected examples use their own
target directory. Go/Web/Python checks passed.
[Current complete native receipt](verification/second-current-checkpoint-20261021/release-rebased-arm64.json).
Previous-head [x86 39543873388](https://github.com/autonomous-ai/lampOS/actions/runs/39543873388)
passed on merge `36f4b77b5f84edbddfff53038a91d8f0b944c66f`, base
`707e02c30cff5d9802ab7e5cfeab77621ca6992d`, with no warnings;
[current receipt](verification/second-current-checkpoint-20261021/release-rebased-clippy.json).
Earlier head `4c847099474a8aca5e6c09d5291a3003ec2a47fd` passed x86
39542671856 ([earlier receipt](verification/second-current-checkpoint-20261021/release-clippy.json));
its ARM64 39542671883 was cancelled by the rebase push. That cancellation is
not a test failure or qualification of the current revision.
Initial head `5b15091dac6c2905704276073dabd3896b88700c` passed ARM64
39540037529: **9,054 nextest passes, one leaky, 59 skipped, four doctests,
12 own-harness tests and relay signals**, plus 29+6 publication checks.
Publication was skipped. The known journal test leak and four shared-example
filename warnings remain recorded. [Initial native receipt](verification/second-current-checkpoint-20261021/release-initial-arm64.json).
That initial run does not qualify the new artifact exporter.

The pending correction names environment `lampos-update-publish`, whose key
must be environment-only and whose deployment rule must permit only the
`lamp-voice` branch. Publish-eligible builds use a fresh target while ordinary
PRs retain the source-time/cache policy. Release staging uses #2481's single
classification: core, optional and provisioning binaries remain; development
binaries and CI examples are excluded after the complete original artifact is
verified. No updater schema or install rule changes. The downloaded previous
ARM64 artifact's checksums all matched: **300,709,350 uncompressed bytes**,
of which **214,974,504 bytes** are the 30 retained binaries. Generated release
units, skills and envelopes add to that subtotal. CI artifacts expire after
seven days; immutable releases and envelope history have no pruning.
[Review correction and measured inventory](https://github.com/autonomous-ai/lampOS/pull/2462#issuecomment-6308606040).

Publication remains off. The job accepts only its successful same-run
lamp-voice push artifact, verifies checksums/uploaded bytes and signed heads,
rejects stale sequences and requires explicit initial bootstrap. It stages
the channel envelope before two non-atomic renames, retains old bytes and
attempts restoration on failure. Interrupted or failed restoration can need
owner repair; no pruning is implemented. Real key provisioning, installed
trial-sequence checks, bootstrap and activation remain owner decisions.
No real release, secret/variable change, bus/service action or device acceptance
occurred. After review/merge, #1812 closes with this workflow/docs slice and
hands wider OTA plus the unperformed real-key internet, quiet recovery and
attended-body acceptance to Migrate. Tests/Migrate's whole-lamp row remains
theirs. [Current handoff reply](https://github.com/autonomous-ai/lampOS/issues/1812#issuecomment-6307681723).

## Previous checkpoint: 2026-10-20

Ready for reviewer integration: yes — both exact heads passed their complete
host gates and automatic ARM64 and x86 Clippy. No lamp qualification is claimed.

Both changes are pushed, one commit each. The [Queue update](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6295022773)
records their heads and counts. #1757 is ready for review after its current CI merge checkout was verified.
Second does not merge either PR.

Music [#1757](https://github.com/autonomous-ai/lampOS/pull/1757),
`fd05557ba14af823863ecc19d738476767bfd77f`, is the allocated music slice of
#1685, based on `dfc5f73c00dbf10a65b668156914d44a19fb4575` after #2157 landed.
Its [complete host gate](verification/second-current-checkpoint-20261020/music-final-host.json)
passed **8,003 / 0 failed / 32 ignored, none filtered**: non-Update
**7,749 / 0 / 31** in **1,247.48 s**, full Update **254 / 0 / 1** in
**192.76 s**. Formatting **4.66 s**, strict workspace/all-target Clippy
**138.77 s**, workspace build **30.98 s** passed. All **29 consumers** and
all **2,114** frozen source inputs are covered. Focused repeats passed
**12** integration, **49** audio and **24** Skills tests. The four renderer
tests, host-debug render, package and disposable install passed, with all
installed hashes matching. Prepared handoff: **199 / 194 microseconds**
first/repeated, confirmation retirement to authored speaker IPC acceptance;
acoustic onset is unmeasured.

The PR preserves current Policy and held-consent gates, search(found), warm
names and delayed Written task news. Recording and provider-speech gaps
must both be zero, and completion must win its atomic transition against
cancellation. Earlier failed preflights remain in the [evidence index](verification/second-current-checkpoint-20261020/README.md).
Music merges cleanly with observed target `14b38dc6a5`; only two docs overlap
its three newer commits. Automatic [ARM64 39451960133](https://github.com/autonomous-ai/lampOS/actions/runs/39451960133)
and [x86 Clippy 39451960115](https://github.com/autonomous-ai/lampOS/actions/runs/39451960115)
passed; their exact tested base and checkout are recorded below.

Policy [#2005](https://github.com/autonomous-ai/lampOS/pull/2005),
`4e1dcb78070d916f4e1acf9729baebe86ad6a490`, refreshes the unchanged test-only
lookup gate on current #1712 base `e26ebab90b1177379a7965cb0da276cdd3fdb153`.
Its [complete 13-consumer gate](verification/second-current-checkpoint-20261020/policy-final-host.json)
passed **5,567 / 0 / 23, none filtered**: **5,317 / 0 / 22** plus full Update
**250 / 0 / 1**. Formatting, strict workspace/all-target Clippy and workspace
build passed, and all **2,074** source hashes match. New automatic
[ARM64 39452151680](https://github.com/autonomous-ai/lampOS/actions/runs/39452151680)
and [x86 Clippy 39452151740](https://github.com/autonomous-ai/lampOS/actions/runs/39452151740)
passed; earlier native results qualify only their recorded heads.

GitHub access is restored. #2157 landed Bedtime/caps at `c4fd23ac2`;
#1732 and #1996 are closed, superseding the older four-foundation publication
plan. The old-CLI fallback and 30-store reset inventory were verified as
carried. The disabled caps RPC still exists and returns unavailable.
The separate [row-52 audit](https://github.com/autonomous-ai/lampOS/issues/1569#issuecomment-6293852079)
identified the issue-time/tick-time race; its owner has #2320, while Tests has
#2318. Music contains no change in that separately owned production path.

Storage cleanup preserved all failures, commits and active/dirty checkouts.
This session removed **31 clean completed worktrees**, **7,426,112 KiB**,
retaining their branches and commits. The verified 49-source archive remains;
a further seven completed source copies were archived with all **38,213**
file hashes checked before removal, reducing **1,273,312 KiB** to
**213,196 KiB**. Completed build caches were removed, and no other builder's
job was stopped. No lamp is assigned or accessed. Music stays off; Bedtime's
physical row remains unqualified.

Native results: Automatic [ARM64 39451960133](https://github.com/autonomous-ai/lampOS/actions/runs/39451960133) passed **8,985 nextest tests**, with **50 skipped** by the existing PR/ignored-test selection, **4 doc tests**, **12 own-harness tests** and the relay-signals check. The PR profile excludes the realtime group; skips are not passes. One passed test is reported leaky (`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`); the build also reports four output-name warnings for the existing duplicate `progress_cost`/`replay_cost` examples. Both findings occur in [earlier run 39298987458](https://github.com/autonomous-ai/lampOS/actions/runs/39298987458), and those files are unchanged by this PR. [x86 Clippy 39451960115](https://github.com/autonomous-ai/lampOS/actions/runs/39451960115) passed with no warnings. Both runs tested merge `7f8d5ee98bb0edf211218fa784a8d0ae336c7b21` over base `14b38dc6a53cc8d792aedacefc0a7cdaef57fb92` for head `fd05557ba14af823863ecc19d738476767bfd77f`.

Automatic [ARM64 39452151680](https://github.com/autonomous-ai/lampOS/actions/runs/39452151680) passed **5,608 nextest tests**, with **25 skipped** by the existing PR/ignored-test selection, **4 doc tests**, **12 own-harness tests** and the relay-signals check. The PR profile excludes the realtime group; skips are not passes. One passed test is reported leaky (`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`); the build also reports four output-name warnings for the existing duplicate `progress_cost`/`replay_cost` examples. Both findings occur in [earlier run 39298987458](https://github.com/autonomous-ai/lampOS/actions/runs/39298987458), and those files are unchanged by this PR. [x86 Clippy 39452151740](https://github.com/autonomous-ai/lampOS/actions/runs/39452151740) passed with no warnings. Both runs tested merge `81b029e76f5d9902dffe8854bf8f963bc36bc3b2` over base `e26ebab90b1177379a7965cb0da276cdd3fdb153` for head `4e1dcb78070d916f4e1acf9729baebe86ad6a490`.

The four completed final-gate build caches were removed after their
processes exited successfully, reclaiming **11,976,604 KiB**. All source
copies and logs remain. Own scratch usage was measured at **6,163,736 KiB**, below the
20 GiB cap. The original workspace remains clean.

## Previous checkpoint: 2026-10-19

Latest reads: all **805 Queue comments** at **20:07 UTC**, and all comments
and review threads on the four own open PRs at **20:10 UTC**. No new own
review comments or unresolved review threads were found. #2026 has merged;
#1712 was observed at `db91c05d398751292936ad33c61a5b5f472f4ac8`.
#1973 remains open, so the target remains `first/voice-cascade-gemini`.
The earlier **19:31 UTC** read found: #2005 has a
clean peer read; its existing upper timing bound is the reviewer's unchanged
P3. #2007, #2008, #2016 and #2017 have merged into #1712; the observed tip
is `2a2d1c5ec`. #1973 is still open, so the target remains #1712.

The clean caps rebase `3ad17c000` on `175e411c17` passed its complete
[27-consumer gate](verification/second-integration-checkpoint-20261019/caps-rebased-host.json):
**6,768 passed, 0 failed, 30 ignored, none filtered**. Non-Update was
**6,636/0/29** in **1,093.04 s**; full Update **132/0/1** in **94.26 s**.
Formatting **3.45 s**, strict workspace Clippy **124.81 s**, and workspace
build **24.21 s** passed. All **1,589** hashes match both copies. The
[changed-name scan](verification/second-integration-checkpoint-20261019/caps-rebased-consumers.json)
covers six changed crates and 27 consumers. This replaces the documentation
conflict; it does not inherit the old head's native qualification.

Rest/Say preparation `f0fbb20e7b0684792b9510801851f5dce7008cc9` on
`3ad17c000` moves the explicit zero-listeners Say assertion into its own
foundation and passed its fresh complete
[27-consumer gate](verification/second-integration-checkpoint-20261019/rest-rebased-host.json):
**6,818 passed, 0 failed, 30 ignored**, none filtered. Non-Update passed
**6,686/0/29** in **1,138.56 s**; full Update **132/0/1** in **99.50 s**.
Formatting **3.55 s**, strict workspace Clippy **127.35 s**, and workspace
build **24.45 s** passed. All **1,591** frozen source hashes match both the
checkout and source-only copy. The
[changed-name scan](verification/second-integration-checkpoint-20261019/rest-rebased-consumers.json)
covers four changed crates and 27 consumers. New automatic ARM64 remains
pending publication; earlier native results qualify only their recorded heads.

First's **19:36 UTC** request is to publish Rest/Say (B), scheduled Skills
(C), and authored Bedtime (D) as stacked PRs now, without waiting for each
predecessor to merge. All four refreshed foundation host gates have passed.
Scheduled Skills `f56f8c4cc` passed **4,935/0/20**, none filtered: non-Update
**4,801/0/19** in **834.52 s**, full Update **134/0/1** in **96.32 s**, formatting
**3.83 s**, strict workspace Clippy **121.14 s**, and workspace build **68.10 s**. All
**1,596** source hashes match both copies. [Host
record](verification/second-integration-checkpoint-20261019/routine-rebased-host.json),
[consumer
scan](verification/second-integration-checkpoint-20261019/routine-rebased-consumers.json).
Publication remains blocked,
and no push or comment is claimed.

Authored Bedtime `0372194d1` passed **4,670/0/20**, none filtered: the ten-crate
reverse-consumer gate passed **4,588/0/20**, and the additional grep-only Actions
consumer passed **82/0/0** in **40.28 s**. Non-Update **4,453/0/19** took **805.26 s**;
full Update **135/0/1** took **97.36 s**. Formatting **3.96 s**, strict workspace Clippy
**124.00 s**, and workspace build **67.44 s** passed. All **1,599** hashes match both
copies. [Host
record](verification/second-integration-checkpoint-20261019/authored-rebased-host.json),
[consumer
scan](verification/second-integration-checkpoint-20261019/authored-rebased-consumers.json).

Cleanup removed **21 completed target directories**, measured by `du` at
**69,904,036 KiB**; `df` then reported **138 GiB free**. Sources, logs and
evidence were retained, and active target directories were kept.

The [peer re-read](verification/second-integration-checkpoint-20261019/peer-fixes-reread.json)
confirms #2008's configuration P2 is fixed in actual merge `d99eee947`: both
worker activation and Engine refuse a missing scoped builder. #2020 fixes
the original settled-writer remember/forget/remember sequence; the reviewer's
already-reported queued-writer race remains. These are source reviews, not
independent runtime or lamp tests. Local GitHub REST/GraphQL and Git transport
still fail intermittently at TLS; the connector can read but refuses comment
writes. Prepared comments, local log commits and foundation publication remain
unconfirmed remotely. No automatic approval rejection is involved.

The **20:06 UTC** [merged-source
re-read](verification/second-integration-checkpoint-20261019/forget-queued-writer-reread.json)
confirms #2020 still has a P2 queue race: an older successful append can restore deleted
IDs after the foreground clear. A full queue drops End, and even an accepted End leaves
a window before it is consumed. This is static evidence; the existing regression does
not synchronize either interleaving. The prepared follow-up is not posted.

A later [file-level preview](verification/second-integration-checkpoint-20261019/foundation-upstream-preview.json) against
`db91c05d3` verifies all 11 overlapping upstream blobs: nine files merge
cleanly, while the reset-store count and coordinator test insertions need
resolving. Preview resolutions preserve 28 person stores and both test
blocks. The frozen green heads remain unchanged; actual fetch/rebase and
applicable gates are still required. [Music integration review](verification/second-integration-checkpoint-20261019/music-later-integration-review.json)
records the required `make_skill` fixture field and later regression controls.
GitHub's separate review endpoint also returned permission denied at 20:18 UTC;
no follow-up review was posted.

Ready for integration: no (#1996 clean rebase passed the complete host gate; publication and automatic ARM64 for the new head are pending)

Ready for integration: no (#1732's four refreshed foundation host gates passed; publication and new automatic ARM64 remain pending)

Ready for integration: no (#1757 follows Bedtime; warm-listener rehearsal passed focused gates; final full and ARM64 gates remain)

Ready for integration: yes (#2005 Policy fixture repair passed host and automatic ARM64; reviewer merge pending)

Completed: #1966 merged into #1712 at 16:15 UTC as `df722db2d0` after local and automatic ARM64 gates passed

The active work is #1672, Bedtime and Morning, followed by the allocated
requested-music slice of #1685. Queue updates go to
[#1722](https://github.com/autonomous-ai/lampOS/issues/1722), because #705
reached its comment limit. No lamp is assigned or accessed. First owns
lamp qualification and merging.

The [14:30 UTC checkpoint](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6269677751)
acknowledges the missed checkpoint and the new second-route requirement.
The full Queue and all four own PR review threads were reread at 18:32 UTC
(737 Queue comments; no reviews, inline comments or unresolved threads on
#1732, #1757, #1996 or #2005). The integration tip at that read was `175e411c17`;
#1925, the separate #1976 stress-bar fix, #1966 Tasks fix, #1990 motion
fixture amendment, #1940, #2002, #2003 and #1967 are merged. Foundation preparation starts at `df722db2d0`. The
Bedtime publication is based on #1712's `b919291458`: Deepgram, DeepSeek and
ElevenLabs. No provider session or device is started by these host tests.

Bedtime's corrected P3 source `6d364e3af` passed the complete
[43-crate host gate](verification/second-integration-checkpoint-20261019/bedtime-followup-gate.json):
**7,651 passed, 0 failed, 44 ignored**, none filtered, plus **12 separate
harness tests**. Formatting **3.50 s**, strict workspace/all-target Clippy
**23.53 s**, non-Update tests **7,517/0/43** in **1,251.19 s**, complete
Update tests **134/0/1** in **73.40 s**, and workspace build **17.77 s**
passed. All **1,629** frozen source files and **156** non-evidence documents
matched. Evidence-only publication amendment `829ba793d6` is saved locally.
These counts qualify that source, not the subsequent second-route port.

The three P3s bound admitted Rest through clock-trust loss, log missing
trust once per interval, and discard stale same-episode renewals while
retaining the exact prior lease. Final leases respect the original sampled
window and the 24-hour cap. The [earlier interrupted attempt](verification/second-integration-checkpoint-20261019/bedtime-followup-stopped.json)
remains separate; its partial test log is not a passing result.

Bedtime is published as `22c9c730bd2def711d0099f2c3b54b26af8e6765`,
one task commit on `b919291458`, targeting `first/voice-cascade-gemini`.
The [publication preflight](verification/second-integration-checkpoint-20261019/bedtime-publication-preflight.json)
passed formatting **3.74 s**, strict workspace/all-target Clippy **125.64 s**,
four second-route cases each **1 passed, 0 failed, 0 ignored; 2,261 filtered**,
all Skills tests **220/0/0**, none filtered, **82.82 s**, and workspace build
**77.38 s**. All **1,592** source hashes matched; publication only amended
verification documents. [Native ARM64 39277565437](https://github.com/autonomous-ai/lampOS/actions/runs/39277565437)
passed **7,796 nextest tests (1 leaky), 48 skipped**, plus **4 doc and 12
separate harness tests**. [The native record](verification/second-integration-checkpoint-20261019/bedtime-publication-arm64.json)
identifies merge checkout `f6ce1cc4db8b304dd9a5608cc4df213bc391b93e` against
`74ea4d028b02e3122e20eb1dc5a66b7a0c3120e8`; release, relay signals, binary
hashes and upload passed. First's 23:09 scope review keeps #1732 draft:
the authored skill must be separated from shared runtime foundations.
The current 90 files comprise 26 verification files (25,678 added lines),
50 runtime/test files (12,926 added, 246 removed), 11 product documents
(775 added, 27 removed), and three skill files (109 added). There is one
commit directly on `b919291458`, with no work-log history in its ancestry.
The [publication archive](verification/bedtime-morning-publication-archive-20261019/archive-manifest.json)
preserves the 26 original evidence files while the dependency split is
prepared. The green counts qualify the original source, not a future split
head. The second-route checks cover
exact optional words with reminders both ways, local morning PCM without
provider input, expiry and the unchanged quiet gate. No cloud or acoustic
result is claimed.

The inherited Tasks diagnostic race is isolated in [#1966](https://github.com/autonomous-ai/lampOS/pull/1966),
head `b7e32343985c66ad5081a9ee98f8b0a68bcd94e4`, and is absent from Bedtime.
The existing fixture now waits for both invalid targets and recovered
sign-in before its unchanged message assertions. The full Tasks gate passed
**179/0/0**, none filtered, **39.94 s**; formatting **3.66 s** and strict
all-target Tasks Clippy **13.33 s** passed. The race is row 36 of #1569.
[Its native run](https://github.com/autonomous-ai/lampOS/actions/runs/39276235123)
now has a terminal successful conclusion. [The exact evidence](verification/second-integration-checkpoint-20261019/tasks-diagnostic-sync-arm64.json)
records **7,595 nextest passed (1 leaky), 48 skipped**, plus **4 doc** and
**12 separate harness tests**. It tested GitHub merge checkout
`910e7931e7eaadbdc0f05bcd1b944e02747b0950` against base
`b919291458c3618838c5f4b56520cf3b2cd739a5`, not the later integration tip.
Release, relay signals, binary hashes and artifact upload passed. Four
existing example filename-collision warnings remain recorded. First merged the PR into #1712 at **16:15 UTC** as
`df722db2d0ee6cb9cd46640e8fc2554065333d13`.

Earlier full host attempts remain failures. The [first attempt](verification/second-integration-checkpoint-20261019/bedtime-second-route-full-first-failure.json)
on `08d9177cb4` stopped at that Tasks race. The [combined preparation attempt](verification/second-integration-checkpoint-20261019/bedtime-combined-policy-failure.json)
on `f21d697ce6`, before the split, later stopped in Policy's
`a_slow_lookup_is_not_waited_for_past_the_deadline_and_is_not_begun_twice`
with the fixture panic `a whole request`; its Policy summary was **164
passed, 1 failed**. Neither partial run is counted as a passing full gate.
The [Tasks-runtime preflight](verification/second-integration-checkpoint-20261019/bedtime-second-route-race-fixed-preflight.json)
was preparation only and does not qualify the final split publication.

Queued behind #1732: Music's preceding current-route preparation is `4a0d423270` on
`f21d697ce6`; it will follow Bedtime's actual landing. A regression reproduced
MORE/decline room accounting with audio both off and on. New acted bookkeeping
now applies only to emitted, valid private recording offers, preserving
ordinary lookups. The [preceding preflight](verification/second-integration-checkpoint-20261019/music-second-route-fixture-failure.json)
passed formatting **3.72 s**, strict workspace/all-target Clippy **24.91 s**,
all **235 second-provider tests**, and **46 requested-audio tests**, zero
failures or ignores (overlapping filters; 2,073 and 2,262 filtered).
Two accepted-decline negatives then failed during fake-peer shutdown: macOS
rejected timeout setup on a socket whose wake peer had already closed.
The fixture now retains that wake connection through the worker join,
without changing any deadline or assertion. [The replacement preflight](verification/second-integration-checkpoint-20261019/music-second-route-preflight.json) passed formatting **3.74 s**, strict workspace Clippy
**26.05 s**, all **235 second-provider**, **46 requested-audio**, **three
accepted-decline** and **one prior-action** cases, plus the complete Skills
suite **239/0/0**, none filtered, **84.28 s**. All **1,608** source hashes
matched. The overlapping filters are not a complete workspace gate. Old full/native
results below qualify only their stated revisions. No provider session,
playback or device access.

The published-Bedtime preparation is `253baed5dacc105c43f102bc2d8a5dd817d8bdd5`,
one Music commit above `22c9c730bd`. [Its integration review](verification/second-integration-checkpoint-20261019/music-publication-integration-review.json)
records preservation of both deltas in six shared files and the worker's
combined audio/clock typed-isolation fixture. Formatting **3.63 s** and strict
workspace/all-target Clippy **56.66 s** passed. The [full host attempt](verification/second-integration-checkpoint-20261019/music-publication-motion-failure.json)
stopped after **1,209.72 s** at the second-route motion fixture's **1 nods**
assertion, already reported on the same base and addressed by #1976. Completed
summaries total **6,891 passed, 1 failed, 43 ignored**, no filters; Voice's
own summary is **2,309/1/12**. Update and build were not reached. The failing
function and Expression speaking implementation match the integration base;
all **1,609** source hashes still match. This remains a failed gate, separate
from the replacement work; that source predates the separate motion fix.

The newer local preparation is `6d57d9c131d1ca0ded0e0f18661c4b22be8259a7`,
one Music commit above Bedtime preparation `b48f7660c7` on `74ea4d028b`.
[Independent source review](verification/second-integration-checkpoint-20261019/music-current-route-integration-review.json)
found no actionable P1/P2 and verified both parents' behavior. New cross-cases
require raw current speech before carried words or the synthetic name fallback,
retain real speech with the Called note, reuse matching cut-in preparation,
and invalidate it when a later note changes context. The scan covers five
changed crates, the 27-crate consumer closure and 142 files; all 198 changed
document links resolve.

The [first new preflight](verification/second-integration-checkpoint-20261019/music-current-route-sandbox-failure.json)
passed formatting **3.86 s** and strict workspace/all-target Clippy **59.05 s**.
All three new or extended cross-case tests passed, but local fake-peer socket
binds were denied by the host sandbox: the outer second-provider summary is
**224 passed, 29 failed, 0 ignored; 2,075 filtered**. Child-process failure
summaries are not added to that total. [The unchanged-source rerun](verification/second-integration-checkpoint-20261019/music-current-route-preflight.json)
with local Unix and loopback sockets passed: formatting **3.91 s**, strict
workspace/all-target Clippy **25.17 s**, **253 second-provider tests**
(2,075 filtered), **46 requested-audio tests** (2,282 filtered), **3
accepted-decline tests** (2,325 filtered) and **1 prior-action test**
(2,327 filtered), all with zero failures or ignores. The complete Skills
suite passed **239/0/0**, none filtered, in **81.43 s**. All **1,609**
source hashes matched. These Voice filters overlap and are not summed into
a full-suite total. The [full host attempt](verification/second-integration-checkpoint-20261019/music-current-route-motion-failure.json)
then stopped after **1,190.43 s** on the inherited second-route **1 nods**
assertion. Completed summaries total **6,899 passed, 1 failed, 43 ignored**,
none filtered, plus **12 separate harness tests**; Voice is **2,315/1/12**
in **314.67 s**. Update and build were not reached. All **1,609** frozen
source hashes still match, and the failing function and Expression speaking
implementation match `74ea4d028b`. The separate #1976 correction has since
merged into #1712; this frozen source predates it. This is a failed gate,
not a completed passing workspace run. No unchanged-source rerun is planned.

First approved the four-part Bedtime sequence at 16:32 UTC and moved the
lamp trial to after its actual landing. Four one-commit changes now
form the sequence: output caps `e1fdc20374f638899e612ef0967d3d414c29708a`
on `df722db2d0`; bounded rest/Say `0bbbe5234d0df0d7b3cc99ab8c6a8195a638485f`;
scheduled Skills `fd31d9c98e283089253b063d6181010846da2741`; and authored
Bedtime `7f4cc5c2674afcd0b9cdacd2e93ec0eb42bec733`. Their individual scopes
are 28, 19, 26 and five files. The last commit contains only the three skill
folder files, the install list and the installer test. Shared interface
tests, private state/reset coverage and disabled switches stay with the
owning runtime change. All original verification documents are archived on
this log branch. The original native result does not qualify these heads.

The [caps foundation host attempt](verification/second-integration-checkpoint-20261019/caps-foundation-motion-failure.json)
passed formatting **3.76 s** and strict workspace/all-target Clippy
**60.46 s**. The 27-crate consumer gate stopped after **956.83 s** at the
inherited second-route motion assertion **1 nods**. Completed summaries
are **5,806 passed, 1 failed, 29 ignored**, none filtered; Voice itself is
**2,256 passed, 1 failed, 12 ignored**, **316.73 s**. Base `df722db2d0`
already contains #1976. The complete second-route test file and Expression
speaking implementation/test blobs match that base. All **1,583** frozen
source hashes match. This is a failed gate. The [previously unreached checks](verification/second-integration-checkpoint-20261019/caps-foundation-unreached.json)
now pass: remaining Voice **243/0/0**, **53.55 s**; Voice-eval **546/0/0**,
**143.95 s**; docs **1/0/0**, **5.24 s**; complete Update **129/0/1**,
**67.28 s**; and workspace build **25.40 s**. No filters, and every frozen
source hash matches in both the checkout and source-only copy. These
results preserve the original failure. First's 16:57 UTC direction is to
publish the four parts with that inherited failure disclosed. The motion
owner's test-only #1990 merged at 17:16 UTC as `3a66c1263b`. Its latest
revision `fb70cdad27` adds separate topic recorders. The [completed integration
ARM64 run 39292881105](verification/second-integration-checkpoint-20261019/integration-motion-amendment-arm64.json)
qualifies the amendment at `3a66c1263b`: **7,641 nextest passed (1 leaky),
48 skipped**, plus **4 doc and 12 separate harness tests**. Release, relay,
hashes and upload passed. That integration result does not qualify the
older private Music source.
Caps is now [#1996](https://github.com/autonomous-ai/lampOS/pull/1996),
head `e1fdc20374`, into `first/voice-cascade-gemini`. Its automatic
[ARM64 run 39290958460](https://github.com/autonomous-ai/lampOS/actions/runs/39290958460)
completed successfully: **7,684 nextest passed (1 leaky), 48 skipped**, plus
**4 doc and 12 separate harness tests**. [The exact native record](verification/second-integration-checkpoint-20261019/caps-foundation-arm64.json)
identifies GitHub merge checkout `2fe03a67f4d7b7268d1e77663e267438a2868e31`
against `1b98d6de325592a04c2fb1d56f1c7da10f5402a1`. Release, relay signals,
binary hashes and upload passed. Four inherited example filename-collision
warnings remain recorded. The other three split heads remained local and
unpushed at that checkpoint. First's later 19:36 UTC request supersedes the
predecessor-merge wait: publish B/C/D as stacked PRs without waiting for merges.

The four-part composition at `7f4cc5c267` passed formatting **3.87 s**,
strict workspace/all-target Clippy **59.56 s**, all five foundation crates
(Expression, Msgs, Schedule, Skills, Setup) **927/0/0**, none filtered,
**157.01 s**, and Say **37/0/0** (2,485 filtered across all targets),
**51.12 s**. Each of the four second-route Say/hum/expiry cases passed
**1/0/0**, 2,278 filtered. These selected cases are not a full consumer gate.

The [first installer attempt](verification/second-integration-checkpoint-20261019/split-composition-initial.json)
failed compilation: Cargo treated shared Msgs and Schedule artifacts as
fresh although their compiled interfaces lacked the exports present in
both source copies. The [unchanged-source isolated checks](verification/second-integration-checkpoint-20261019/split-composition-isolated.json)
passed with separate fresh target directories: complete Update **132/0/1**,
none filtered, **107.68 s**, and source-only workspace build **143.71 s**.
All **1,593** frozen source hashes match. The failed cache attempt stays
recorded. Subsequent per-head gates use fresh, separate checkout and
source-only targets. [Rest/Say's complete 27-crate gate](verification/second-integration-checkpoint-20261019/rest-foundation-host.json)
passed **6,776/0/30**, none filtered: non-Update **6,647/0/29** in
**1,224.96 s**, full Update **129/0/1** in **105.20 s**, formatting
**3.94 s**, strict workspace Clippy **126.14 s**, build **24.79 s**.
All **1,585** frozen source hashes match. [Scheduled Skills' 13-crate gate](verification/second-integration-checkpoint-20261019/routine-foundation-host.json)
passed **4,898/0/20**, none filtered: non-Update **4,767/0/19** in
**896.43 s**, full Update **131/0/1** in **111.26 s**, formatting
**4.06 s**, strict workspace Clippy **133.45 s**, build **70.08 s**.
All **1,590** frozen source hashes match. Both remain local pending the
preceding foundation's actual merge, then their own automatic ARM64 run.

Authored Bedtime's [complete remaining consumer command](verification/second-integration-checkpoint-20261019/authored-foundation-host-failure.json)
reported **4,418 passed, 1 failed, 19 ignored**, none filtered,
**808.28 s**; Voice itself is **2,266/1/12**, **325.44 s**. The inherited
second-route **1 nods** assertion is the only failure. Its entire fixture
blob matches parent `fd31d9c98e`, and the failing function matches the
foundation base `df722db2d0`. Its fresh strict workspace Clippy passed
**88.20 s**; the same-source full Update **132/0/1** and build results
above remain valid. Together the selected consumer suites report
**4,550/1/20**. This remains a failed gate, disclosed under First's 16:57
publication direction; no unchanged-source retry replaces it. The later
#1990 motion correction is absent from this frozen head.

Music is committed privately as `c44d66557b` above the four split commits,
preserving ScopedSystem, notes-aware preparation, bounded TTS and shutdown.
Independent review of the other 47 changed files found no P1/P2; the two
documentation links it identified are corrected. Additional cross-cases
cover scoped identity, revocation after preparation, empty speech and Stop
through the real worker with the Music runner. The final three-file review
also found no P1/P2 and verified all parent tests remain.

The [first foundation preflight](verification/second-integration-checkpoint-20261019/music-foundations-teardown-failure.json)
on `c82c04d6df` passed formatting **4.12 s** and strict workspace/all-target
Clippy **130.92 s**, then reported **258 passed, 1 failed, 0 ignored; 2,083
filtered**, **7.42 s** test time. The new held-RPC Stop fixture had passed
its scope-revocation, no-second-chat and **300 ms** shutdown assertions,
then failed its final receive on macOS's closed-peer `ConnectionReset`.
The fixture now follows the existing terminal-notice contract: check every
queued event, accept only that closed-peer error, and restore global thread
state even during unwinding. No production code or bounds changed. The
[replacement preflight](verification/second-integration-checkpoint-20261019/music-foundations-preflight.json)
passed formatting **4.13 s**, strict workspace Clippy **137.67 s**, the exact
held-RPC Stop case **1/0/0** (2,341 filtered), all **259 second-provider**
(2,083 filtered), **46 requested-audio** (2,296 filtered), **3 accepted-decline**
(2,339 filtered), and **1 prior-action** (2,341 filtered) cases, all without
failures or ignores. Complete Actions, Msgs and Skills passed **486/0/0**,
none filtered, **143.54 s**; the full ring-worker integration suite passed
**23/0/0**, **15.42 s**. Workspace build passed **82.97 s**. All **1,610**
frozen source hashes match in checkout and source-only copy. Voice filters
overlap and are not a full-suite total. The [complete 27-crate consumer gate](verification/second-integration-checkpoint-20261019/music-foundations-consumers-failure.json)
then finished with **6,980 passed, 2 failed, 30 ignored**, none filtered.
Non-Update tests reported **6,848/2/29** in **1,099.31 s**; full Update passed
**132/0/1** in **100.82 s**. The failures were Policy's `a whole request`
(**164/1/0**, **10.05 s**) and the inherited motion fixture's `1 nods`
(Voice **2,329/1/12**, **320.20 s**). Both entire fixture blobs match parent
`7f4cc5c267`; the Policy blob also matches current integration `e9d84fbee7`.
All **1,610** source hashes still match both copies. This is a failed gate,
with no hidden filters or unchanged-source retry. Music remains queued
behind actual Bedtime landing and a final port preserving newer changes.

The separate [Policy fixture repair #2005](https://github.com/autonomous-ai/lampOS/pull/2005)
is head `0040b45576a8d467a4925523b26dfad102c6e416` on `e9d84fbee7`.
It gates the first lookup through both unchanged 120 ms deadline checks,
then releases it. The wait is bounded and disconnects on unwind; the
single-lookup, complete-request, cache and re-resolution assertions remain.
Only a private test fixture changes. [Its complete host gate](verification/second-integration-checkpoint-20261019/policy-lookup-host.json)
passed **264/0/2**, none filtered, **73.90 s**; library **165/0/0**.
Formatting **3.89 s**, strict all-target Policy Clippy **16.28 s**, and
build **0.11 s** passed. [Changed-name grep](verification/second-integration-checkpoint-20261019/policy-lookup-consumers.json)
matches only that private test module. All **1,584** frozen source hashes
match both copies. [Automatic ARM64 39298987458](https://github.com/autonomous-ai/lampOS/actions/runs/39298987458)
completed successfully: **7,651 nextest passed (1 leaky), 48 skipped**,
plus **4 doc and 12 separate harness tests**. [The native record](verification/second-integration-checkpoint-20261019/policy-lookup-arm64.json)
identifies merge checkout `2f942ada51eebaf4ac387cde31852242be8348e7` against
base `e9d84fbee72731300594a0762eae7a9ba33040a5`. Release, relay signals,
binary hashes and artifact upload passed. Four inherited example filename
collision warnings remain. #2005 is ready for review. This separate repair
is not bundled into Music or a Bedtime foundation.

The final Music port must also preserve the held-report scope correction.
The [reported P2](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6273396615)
finds `done_though_held` using the sticky session's builder despite holding
an admitted lease. First assigned the upstream repair and privacy review;
the regression must use differing sticky and admitted people. The source
call chain was verified statically; no device reproduction is claimed.

The three later Bedtime boundaries were checked against `e9d84fbee7`:
[Rest/Say](verification/second-integration-checkpoint-20261019/rest-integration-boundary.json),
[scheduled Skills](verification/second-integration-checkpoint-20261019/routine-integration-boundary.json)
and [authored Bedtime](verification/second-integration-checkpoint-20261019/authored-integration-boundary.json).
Their overlapping files merge cleanly while retaining both sides' tests,
the newer private `second.json` installation, current service defaults and
disabled routine switches. These are textual/source checks, not new runtime
gates on a combined head.

The [conditional Music review](verification/second-integration-checkpoint-20261019/music-conditional-integration-review.json)
checks the still-unmerged #2007 memory note and #2008 speaking-face heads.
It found a separate [activation P2 in #2008](https://github.com/autonomous-ai/lampOS/pull/2008#issuecomment-6274090141):
face narrowing can be enabled without the scoped chat builder, leaving
personal memory/history in the prompt even after memory calls narrow to
room scope. The owner has the finding and requested activation regression;
no runtime reproduction or peer source change is claimed. Music has no
pre-final offer on this path: prepared chat buffers events without running
tools, and final admission uses the live lease. Conditional regression
cases are recorded for whichever revisions actually land.

The private warm-listener rehearsal is `90faee56fa4464f270b9ac12d40fca0ee8c0d307`
on `52a42b1eb5` plus the four Bedtime foundations. Its [source review](verification/second-integration-checkpoint-20261019/music-current-integration-review.json)
confirms the Engine/worker union retains carried-answer restrictions, scoped
calls, busy-room Policy waiting and private-offer checks. Five new regressions
cover exact transcripts, current request identity, spare replacement, typed/Say
isolation, prepared admission and Stop during a pending Music RPC and warm STT
connection. All inherited helper/function names remain.

The [focused preflight](verification/second-integration-checkpoint-20261019/music-current-integration-preflight.json)
passed formatting **3.64 s**, strict workspace/all-target Clippy **127.15 s**,
**278 second-provider**, **46 requested-audio**, **3 accepted-decline** and
**1 prior-action** cases; these filters overlap. Full Actions/Msgs/Skills
passed **486/0/0**, none filtered, **124.16 s**, ring-worker **23/0/0**, and
workspace build **77.12 s**. All **1,616** source hashes match both the checkout
and source-only copy. The [27-consumer scan](verification/second-integration-checkpoint-20261019/music-current-integration-consumers.json)
records the final full gate's scope; this preflight is not that gate.
The [initial sandbox attempt](verification/second-integration-checkpoint-20261019/music-current-integration-sandbox-failure.json)
reported **244 passed, 34 failed** when local socket creation was denied.
The identical frozen source/target continued with socket access; that failure
is preserved, and formatting/Clippy were not repeated.

First subsequently reported #2002's warm-STT pause/split P1 on the lamp:
a name-only fragment followed by a pause could leave both fragments held.
Its owner is fixing it and the switch remains off on lamps. The above Engine
and local IPC checks do not qualify live STT or acoustic behavior. The further private composition on `175e411c17` is
`b59fc17757b6f6dc8f0138c8ac1da077fff06726`. Its [source review](verification/second-integration-checkpoint-20261019/music-unclear-integration-review.json)
retains both parents' production deltas and test helpers. Three new cases
prove an unclear final line cannot dispatch a prepared Music call, the hum
note invalidates an otherwise identical prepared prompt, and a held successor
cancels pending Music while its carried answer receives no recording grant.
The real Written retry instruction cannot authorize a recording either.
[Its focused gate](verification/second-integration-checkpoint-20261019/music-unclear-integration-preflight.json)
passed formatting **3.61 s**, strict workspace/all-target Clippy **118.51 s**,
all **283 provider** and **34 second-route coordinator** cases, complete
Memory **218/0/5**, none filtered, **83.83 s**, and workspace build **78.15 s**.
All **1,616** frozen source hashes match both copies. These checks do not
replace the final 27-consumer/native gate or clear the uncovered Memory P2. Final Music publication follows the
actual Bedtime landing and preserves the separately owned held-report and
speaking-face fixes when they land.

Review of the newer #2003 Memory change found a [separate source-verified P2](https://github.com/autonomous-ai/lampOS/pull/2003#issuecomment-6274506329):
after forget deletes a kept fact's exchange, the client can send that deleted
source ID on the next remember call in the same conversation, causing the new
fact to be refused. The existing real-client/store test ends after deletion;
the owner has the requested follow-on-write regression. No runtime or lamp
reproduction, peer source edit, or fix ownership is claimed here.
First assigned that repair to the Memory owner at 18:40 UTC, with both real
client/store follow-on-write regressions.

GitHub reads and the own-log push began failing with TLS timeouts/connection
reset around 18:43 UTC. Local work and bounded read-only retries continue;
no remote publication or latest review state is assumed from those failures.
A local merge check also found #1996 now conflicts only in the switch table,
beside #1967's updated unclear-line description. A documentation-only attempt
did not remove the conflict under Git's actual merge algorithm and is not
published. The clean rebase `3ad17c000` preserves both rows and passed the
complete host gate recorded above. First's later 19:36 UTC request is to
publish B/C/D as stacked PRs now; predecessor merges are no longer a publication
prerequisite. Connectivity still blocks confirming publication.


Bedtime and Tasks have only their original automatic PR ARM64 run,
following First's no-duplicate-dispatch direction. GitHub's [Actions incident](https://www.githubstatus.com/incidents/8tzbpcnl75jp)
reported delayed workflow processing at 15:56 UTC. Tasks showed successful
steps before its run and check finally reported success; the exact evidence
is recorded above. Bedtime has now also concluded green, with its exact
merge checkout recorded. It remains draft for the requested scope split;
Music remains queued for actual landing and final native qualification.

The full Bedtime read returned four P2s and a topic-registration gap. This
PR was returned to draft. Matching binary/unit installation is now documented;
product reset clears both durable directories; all three bus topics are
registered with additive field compatibility and strict enum values. The new
schema records exact dates under a 1 MiB bound, and scheduled work requires
kernel clock trust or an explicit development override. Rest can use its
issued three-second lease through a short observation gap, but cannot renew
without fresh matching evidence. Unrelated definition reload preserves only
already-admitted rest whose complete enabled owner definition is unchanged.
Independent review found no remaining P1/P2 in that lease/reload scope.
The corrected [replacement preflight](verification/second-integration-checkpoint-20261019/bedtime-review-preflight-v3.json) passed formatting (**3.59 s**), strict all-target Clippy
for Skills/Msgs/Setup (**8.82 s**) and their complete suites: **410 passed,
0 failed, 0 ignored**, none filtered (**82.33 s**). The fixes are committed
as `c2dea42988205689e00557a72405e53bffae9aaf` before the latest-base rebase.
The combined source is now `5e83af10210cb1a4bfbeb15887274d1e519fe4c8`, one
task commit on `4a188e789`, retaining merged nudge limits and held colors.
Its new daily/budget/trust regression is included in the replacement
[complete 43-crate gate](verification/second-integration-checkpoint-20261019/bedtime-review-gate.json):
**7,608 passed, 0 failed, 44 ignored**, none filtered, plus **12 separate
own-harness tests**. Formatting **3.54 s**, strict workspace/all-target
Clippy **51.20 s**, non-Update suites **7,474/0/43** in **1,260.35 s**,
the entire Update suite **134/0/1** in **77.36 s**, and workspace build
**16.41 s**, all passed. All **27** affected and reverse-dependent crates
are included. All **1,617** source and **156** non-evidence document hashes
matched. Published head `805f569616334c3d467bf7f5f23676d0a3d1bd95` changes only
three verification documents after that freeze; it is pushed and mergeable.
[The point-by-point review response](https://github.com/autonomous-ai/lampOS/pull/1732#issuecomment-6265384739)
is posted. Replacement [native ARM64](verification/second-integration-checkpoint-20261019/bedtime-review-arm64.json),
[39239473411](https://github.com/autonomous-ai/lampOS/actions/runs/39239473411),
passed **7,693 nextest tests (1 leaky), 48 skipped**, plus **4 doc** and
**12 separate harness tests**. It tested GitHub merge checkout
`cbea9ad9dd5a4d35f34f6967c4f257c114d04194` against base
`179016adc0a5a7ab2d6a9e853fe281cedbea3f1c`; release, relay and artifact stages
passed. Existing example filename-collision warnings are preserved in the
record. The [completion response](https://github.com/autonomous-ai/lampOS/pull/1732#issuecomment-6265769529)
was posted for that earlier source. The later P3 follow-ups have returned
#1732 to draft as recorded above. First retains lamp qualification.
The first preflight stopped on one
Clippy warning; the second passed Clippy but stopped on an invalid automatic
rest test fixture. Both [the Clippy failure](verification/second-integration-checkpoint-20261019/bedtime-review-preflight.json) and [the fixture failure](verification/second-integration-checkpoint-20261019/bedtime-review-preflight-v2.json) are retained. None of the old green counts below qualifies these new source changes.

The separate Music integration with #1833 reproduced two accepted-decline
failures: later and parked calls dispatched Music after the decline. The
new guards stop both while preserving prior executed actions and the
consent-withdrawal direction of ReportsStop. The
[negative reproduction](verification/second-integration-checkpoint-20261019/music-decline-before-fix.json)
and [corrected focused preflight](verification/second-integration-checkpoint-20261019/music-decline-after-fix.json)
record exact commands, counts and log hashes. The focused 42 requested-audio,
eight decline, two credit, provider prior-action and verdict tests pass;
these overlap and are not a full-gate total. Independent review found no
P1/P2. The added [ReportsStop-after-decline control](verification/second-integration-checkpoint-20261019/music-decline-reports-control.json) also passes: **3 accepted-decline tests passed**, 0 failed/ignored,
2,147 filtered, in **25.84 s** including compilation; formatting **3.63 s**
and strict Voice Clippy **17.76 s**. The control uses an isolated private
bus, checks Policy holding off/on, and confirms no speech revival.

Merged #1839 is now combined with that decline fix and the latest Bedtime
source in local Music commit `3d2b511dee47a41dd8efead95168546113b95601`.
Scoped context and requested audio remain independent; the matrix covers all
four settings with spoken and Written requests, admitted identity, revoked
context construction, queue credit and Messages isolation. Independent
review found no P1/P2. [The first compile](verification/second-integration-checkpoint-20261019/music-context-compile-failure.json) stopped because the new test requested an unavailable Tokio runtime
feature; no tests ran. The corrected bounded-thread fixture adds no dependency.
[Its focused preflight](verification/second-integration-checkpoint-20261019/music-context-preflight.json) passed formatting **3.76 s**, strict all-target Voice Clippy **17.96 s**,
**44 requested-audio tests** (2,117 filtered), **3 accepted-decline tests**
(2,158 filtered), and separate scope, Messages and revoked-result controls.
Every filter had zero failures/ignores; their counts overlap. All **1,634**
source hashes matched. The [complete 43-crate preparation gate](verification/second-integration-checkpoint-20261019/music-context-gate.json)
passed **7,708 tests, 0 failed, 44 ignored**, none filtered, plus **12
separate harness tests**. Formatting **3.82 s**, strict workspace/all-target
Clippy **57.68 s**, non-Update suites **7,574/0/43** in **1,342.36 s**,
complete Update **134/0/1** in **75.15 s**, and workspace build **18.53 s**,
all passed. Five changed crates and the full 27-crate consumer closure are
covered. All **1,634** source and **157** non-evidence document hashes
matched. Published preparation `fb04a5f5c6c17fdc192222ccf64a909cc831879a`
is one Music commit above published Bedtime `805f56961`; only verification
records changed after the gate. [Native ARM64 39242245705](https://github.com/autonomous-ai/lampOS/actions/runs/39242245705)
passed on that exact preparation head: **7,790 nextest tests (1 leaky),
48 skipped**, plus **4 doc and 12 separate harness tests**. Release, relay
and artifact stages passed. [The native evidence](verification/second-integration-checkpoint-20261019/music-context-arm64.json)
records the exact checkout, log digest and inherited example filename
collision warnings. The [PR checkpoint](https://github.com/autonomous-ai/lampOS/pull/1757#issuecomment-6265778871)
is posted. Final #1757 still follows Bedtime's actual landing. No device
or playback was used.

For #694, the referenced played-line fixtures are absent from this Mac, and
an exact-input request is on the issue. The [supplemental synthetic direct-PCM measurements](verification/second-bare-name-20261019/README.md) are complete: **144 inputs**,
36 distinct utterances, three anonymized English synthetic voices. Warm
catch counts were bare **1/3**, initial **2/3**, trailing **0/3**, and Hey
**3/3**. These do not establish acoustic catch rates or STT onset loss.
All trailing maxima are below 0.10, so a detector threshold change alone
is insufficient for this set. The [posted proposal](https://github.com/autonomous-ai/lampOS/issues/694#issuecomment-6264868883) retains thresholds and requests
the exact played and STT inputs before choosing an onset fix or model work.
First [accepted that proposal](https://github.com/autonomous-ai/lampOS/issues/694#issuecomment-6265313797):
finish Bedtime's P2s, then compare a candidate at unchanged thresholds on
bare/initial/trailing calls and office-like negatives, with false hearings
per hour decisive. The [paired synthetic screen of the historical candidate](verification/second-name-candidate-20261019/README.md)
is complete: **288/288 checker calls succeeded**. Warm primary/candidate
catches were bare **1/3 vs 0/3**, initial **2/3 vs 1/3**, trailing **0/3 vs
1/3**, and Hey **3/3 vs 2/3**; controls were **0/24 vs 2/24** hearings.
That candidate is rejected as a replacement. All 144 fresh-primary results
match earlier printed scores and input timestamps; host timing was not
compared. The controls contain only **33.2419375 seconds** of unique speech,
so they do not qualify false hearings per hour. The pinned-primary office
pilot stopped before any scoring because a verified source has one trailing
NUL after its RIFF chunks. Its failed validation is preserved; no rate or
accepted denominator is claimed. A versioned parser correction was prepared,
but automatic approval review blocked the remaining public download as
outside the trusted coding authorization. Explicit user authorization for
that bounded continuation is pending. The training recipe and held-out
manifest remain text preparation only. No model training, held-out synthesis,
played-line result or device result is claimed.

The due [#1123 checkpoint](https://github.com/autonomous-ai/lampOS/issues/1123#issuecomment-6264970534)
is posted. The merged work and deployed-unit flag were checked in source;
the new lamp comparison and closure remain with First. No new device
observation is claimed.

Native CI results linked to a PR head test GitHub's merge checkout, as
documented in `.github/workflows/lampos-arm64.yml`; a `workflow_dispatch`
run tests the branch head. References below to a PR head identify the
source under review, not necessarily the Git checkout tested by CI.
Different merge bases can produce different test counts even when the
PR-head trees are identical. Each run's recorded counts remain separate.

- **Earlier published Bedtime, [#1732](https://github.com/autonomous-ai/lampOS/pull/1732):**
  pushed head `1d9112d166beeac0a5a1ea96df3c4081b4f549f1`, one task commit
  above `b03cb4a4b3`. The complete local gate covers all 42 crates:
  **7,423 passed, 0 failed, 44 ignored**, none filtered. Formatting passed
  in **3.50 s** and strict all-target workspace Clippy in **54.74 s**.
  Non-Update suites passed **7,293/0/43** in **1,300.23 s**; the complete
  Update suite passed **130/0/1** in **48.76 s**; the workspace build passed
  in **18.10 s**. All **1,549** frozen source hashes matched at completion.
  A later `install.md` paragraph relocation changed documentation only;
  its separate checks passed formatting in **3.63 s**, strict all-target
  Update Clippy in **1.50 s**, and the complete Update suite **130/0/1** in
  **45.21 s**. That repeated suite is not added to the full-gate total.
  The preceding published head `c7f9c90879` passed native ARM64
  [39221703897](https://github.com/autonomous-ai/lampOS/actions/runs/39221703897):
  **7,520 passed (1 leaky), 48 skipped**, plus **4 doc** and **12 own-harness**
  tests; relay, release, checksum and artifact steps passed. Its tested
  merge was `9eeb73f0a019d1fd0c65db5bcd8e8d4704a5988c` against `da0806945e`.
  After upstream `9df25da794`, three unchanged export-fixture records were
  moved to avoid overlap with VoiceTurn. All production code and the other
  **1,548** non-document source files remain unchanged. The follow-up
  passed formatting (**4.22 s**), strict all-target workspace Clippy
  (**26.33 s**) and the complete Voice-eval suite: **540 passed, 0 failed,
  0 ignored**, none filtered (**125.36 s**). These counts remain separate.
  This published head was qualified before the new review fixes. Native
  [39224545431](https://github.com/autonomous-ai/lampOS/actions/runs/39224545431)
  passed **7,597 tests (1 leaky), 48 skipped**, plus **4 doc tests** and
  **12 own-harness tests**. Release, relay, checksums and artifact upload
  passed. It tested GitHub merge `9f866c1f57932db87e0954032e7e66e25e324736`
  against `8fecf7443c985c1fda9adfa5f38bc1e76005fcc8` for the published head.
  [The native record](verification/second-integration-checkpoint-20261019/bedtime-arm64.json)
  preserves the full-log hash and inherited warnings. First retains merge
  and lamp qualification ownership.
- **Earlier Music preparations, [#1757](https://github.com/autonomous-ai/lampOS/pull/1757):**
  the earlier prepared `4bcdf9fee1` on preliminary Bedtime `ceeedf68f3`
  failed its complete gate on its own worker lifecycle test:
  the Voice test binary reported **2,088 passed, 1 failed, 12 ignored**;
  all Rust summaries through the failure total **6,588 passed, 1 failed,
  43 ignored**, none filtered, plus a separate **12/0** own-harness run.
  Update and the standalone workspace build did not run. Cache contention
  from a rejected second offer is reachable, but the failed assertion did
  not print the actual operation, so the failure's exact cause is not yet
  proved. The corrected preparation is `067c05bbb7c6803bd38ba879b961a130f526006f`,
  one Music commit above published Bedtime `c7f9c90879`. Admission reserves
  capacity before reading the cache and releases it on rejection. Two new
  regressions prove baseline rejection returns capacity and cache contention
  cancels before Start without later revival. The focused preflight passed:
  formatting **3.59 s**, strict all-target Voice Clippy **25.46 s**, and
  requested-audio tests **27 passed, 0 failed, 0 ignored, 2,076 filtered**
  (**35.40 s**, including compilation). Its complete 42-crate gate passed
  **7,509 tests, 0 failed, 44 ignored**, none filtered, plus **12 separate
  own-harness tests**. Formatting passed in **3.67 s**, strict all-target
  workspace Clippy in **51.86 s**, non-Update suites **7,379/0/43** in
  **1,261.13 s**, the entire Update suite **130/0/1** in **69.92 s**, and
  the workspace build in **18.21 s**. All **1,566** frozen source hashes
  and **156** non-evidence document hashes matched. [The admission record](verification/second-integration-checkpoint-20261019/music-admission-gate.json)
  preserves these commands, counts and hashes. It remains unpublished and
  queued behind #1732.
  The current preparation is published as `5d815c9f169cc8c82cf0498ee85d02c1c4170e4a`
  on `second/music-scope-integration`, above prepared Bedtime `8706fe851d`.
  It combines admitted Memory scope and VoiceTurn on `9df25da794` plus
  Bedtime. The first focused attempt had a test-local shadowing compile
  error; renaming the variable changed no production code. The preceding
  corrected preflight passed **38 requested-audio tests** before review
  found a final-queue scope gap. Both provider boundaries now recheck a
  queued offer on every send attempt, including after IPC credit returns.
  Two real-IPC regressions cover camera-only revocation with the same
  microphone owner, denial/reopen, restored credit and valid scope after
  generation completion. Independent re-review found no remaining
  actionable issue in that focused scope.
  [The final focused preflight](verification/second-integration-checkpoint-20261019/music-scope-preflight.json)
  passed formatting (**3.55 s**), strict all-target Voice Clippy (**16.32 s**),
  **40 requested-audio tests**, both final-credit tests, and separate scoped
  Memory/routing/verdict controls. The overlapping filters are not added
  together or to the complete gate. Host handoff was **275 microseconds
  first, 329 repeated**, measured to authored speaker IPC acceptance;
  acoustic onset is unmeasured.
  [The complete scope gate](verification/second-integration-checkpoint-20261019/music-scope-gate.json)
  on frozen source `befb83aac5991dae2d3dc572c93c69152bea17d0` passed
  **7,600 tests, 0 failed, 44 ignored**, none filtered, across all **42 crates**,
  plus **12 separate own-harness tests**. Formatting passed in **3.63 s**,
  strict all-target workspace Clippy in **55.50 s**, non-Update suites
  **7,470/0/43** in **1,290.12 s**, the complete Update suite **130/0/1** in
  **69.27 s**, and the workspace build in **17.76 s**. All **1,600** frozen
  source hashes and **157** non-evidence document hashes matched then.
  A subsequent documentation-only clarification separates queued-offer
  scope from playback microphone privacy: camera-only scope revocation
  does not cancel an already accepted playback ticket. All source files
  and the other **156** non-evidence documents remain unchanged; **182**
  relative links checked with none missing. This follow-up is recorded
  separately and changed no runtime code.
  Native ARM64 [39231334336](https://github.com/autonomous-ai/lampOS/actions/runs/39231334336)
  passed by workflow dispatch on the exact published preparation head:
  **7,682 passed (1 leaky), 48 skipped**, plus **4 doc tests and 12 separate
  own-harness tests**; release, relay, checksums and artifact upload passed.
  [The native record](verification/second-integration-checkpoint-20261019/music-scope-arm64.json)
  preserves the source identity and full-log hash. It is preparation evidence,
  not the final one-task PR gate after #1732 lands. Bedtime is now draft for
  the returned review fixes before its coordinated lamp check and merge.
  No device was accessed and
  all Music switches remain off. Earlier counts qualify only their named
  sources; the failures and preceding focused results remain preserved.

Earlier, at 07:58 UTC, both integrations were frozen locally above `b03cb4a4b3`:
Bedtime `ceeedf68f3` on `second/bedtime-rising-guard`, then Music
`4bcdf9fee1` on `second/music-after-rising`. Both full 42-crate gates passed
formatting and strict all-target workspace Clippy; their complete tests
were then running in separate source-only copies. Bedtime's preceding complete
Skills preflight passed **188/0/0**, none filtered. Its new daily-rise
fixture initially failed the private-directory requirement (**187/1/0**);
setting its test directory to mode `0700` fixed it without weakening the
production check. The failed attempt remains recorded. The combined
regressions preserve rise/daily/reload fences, exact speech's original Wake
deadline, Music's string-only manual-call shape, ReportsStop dispatch and
all eight optional-flag combinations. The outcomes above supersede that
in-progress status. Historical counts below qualify only their named heads;
each replacement needs its own complete gate and native result before
readiness.

- **Historical Bedtime stage, [#1732](https://github.com/autonomous-ai/lampOS/pull/1732)**,
  `second/bedtime-morning`: a disabled installed core skill, durable daily
  admission, leased Expression rest/wake, and durable light/volume ceilings.
  Previously published head `30897c3fe10fb9afa5f2efd1b052513056cc0f18` was
  based on `6ea2b8ffe` and merged cleanly with `406e2ca64d`. The earlier complete
  light-scene gate on `b7dbae26f` passed **7,352 passed, 0 failed,
  44 ignored**, across all 42 crates, none filtered out. The subsequent
  complete text-integration consumer gate passed **4,237 passed, 0 failed,
  20 ignored**, across ten crates, none filtered out. Workspace formatting,
  strict all-target workspace Clippy and the workspace build also passed;
  all 1,541 frozen source hashes matched at completion. The
  [host record](https://github.com/autonomous-ai/lampOS/blob/30897c3fe10fb9afa5f2efd1b052513056cc0f18/lampOS/docs/verification/bedtime-morning-host-20261019/README.md)
  keeps the full and subsequent consumer stages separate. Native ARM64
  [39210564668](https://github.com/autonomous-ai/lampOS/actions/runs/39210564668)
  passed on this exact published head: **7,442 passed (1 leaky), 48 skipped**,
  plus **4 doc tests** and **12 own-harness tests**; the relay-signals harness,
  release build, binary checksums and artifact upload also passed. Every
  other PR check passed on that head. It merged cleanly with `406e2ca64d`
  before the newer conflicts described above. The first builder still owns
  its installation minute and merge after the replacement head qualifies.
  Native log SHA-256:
  `5e8761ed754d24095543758bb6d15dbb7e2fe807af6bae536a46b575cba9e8a3`.
- **Historical Music stages, [#1757](https://github.com/autonomous-ai/lampOS/pull/1757)**,
  `second/requested-music`: one spoken confirmation followed by a fixed
  local recording, under a private short lease and the same Voice owner.
  The original 48-second source-rendered example is installable; no WAV or
  binary is committed. Standalone published head `97cdfbea7` passed
  **7,257/0/44** locally across all 42 crates, none filtered. Its native ARM64
  [39204794234](https://github.com/autonomous-ai/lampOS/actions/runs/39204794234)
  passed **7,342 tests (1 leaky), 48 skipped**, plus **4 doc tests** and
  **12 own-harness tests**. Those standalone results are historical, not
  qualification of the combined source. The combined light-scene full gate
  passed **7,432 passed, 0 failed, 44 ignored**, across all 42 crates, none
  filtered out, including **108 Actions** and **192 Skills** tests. All
  1,556 frozen source hashes matched at completion. Its subsequent complete
  ten-crate consumer gate passed **4,288 passed, 0 failed, 20 ignored**, none
  filtered. Formatting, strict all-target workspace Clippy and workspace
  build passed; all 1,558 frozen source hashes matched. The prepared Music
  commit `aba4e44a5c916e3577a05181b4eb888dbfcebe5a` on
  `second/music-after-text-guard` was one task commit above then-published Bedtime
  `30897c3fe1`, preserving the typed body-tool refusal, private media
  context/offer handling and matching filtered catalog metadata. The parent
  update changed evidence documents only; source hashes stayed identical.
  [Combined native run 39211390300](https://github.com/autonomous-ai/lampOS/actions/runs/39211390300)
  passed on that exact preparation head: **7,521 passed (1 leaky),
  48 skipped**, plus **4 doc tests** and **12 own-harness tests**. The
  relay-signals harness, release build, binary checksums and artifact upload
  passed. Native log SHA-256:
  `86d7b3c506573de01cc037e325e6e5fe4667361c36d895af951ddecad9ed35c9`.
  This preparation result does not qualify the final PR head. #1757 remains
  on its standalone head until #1732 lands; then its one Music commit will
  target `lamp-voice` directly and receive final exact-head native CI.

The historical source comparison between the full light-scene snapshots and the
text-integrated source identifies runtime/test changes only in Memory,
Skills and Voice. Their complete reverse-consumer closure is ten crates:
Body, Cloud, Expression, Keeper, Lean, Memory, Skills, Update, Voice and
Voice-eval. Both consumer stages passed.
These stages rerun `cargo fmt --all --check`, strict all-target workspace
Clippy and `cargo build --workspace`, with complete suites for the selected
crates. The earlier full stages cover all 42 crates. Counts from different
source snapshots are kept separate and are not added together.

All non-Update suites and the build run from source-only copies. The entire
Update suite runs in its checkout because the inherited image-journal test
reads the parent image fixture; no new ignore or filter hides that portability
limit. Native results qualify their recorded source head and tested checkout.
The historical standalone Music, preceding Bedtime and combined Music runs reported
the existing
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal` leak.
Those Bedtime and combined Music native builds repeated the inherited
example filename collisions for `replay_cost` and `progress_cost` (including
their `.dwp` outputs); these warnings were present in the earlier Bedtime
native run.

Product remains seven installed core skills, six enabled and 31 advertised
functions; Bedtime stays `enabled = false`. `LAMP_SKILL_ROUTINES`,
`LAMP_REST_REQUESTS`, `LAMP_OUTPUT_CAPS` and `LAMP_REQUESTED_AUDIO` stay off in
product and lamp units. Existing user volume, privacy, cancellation and
output bounds remain authoritative. Neither host nor native CI establishes
acoustic, optical or motor behavior. The recommended landing order is #1732
then the rebased #1757; these remain the only two active scopes.

The original peer review requested on the Queue:
[#1782](https://github.com/autonomous-ai/lampOS/pull/1782), exact head
`6428774ff7d4cac9b20948df96ce79701d0b5315`, was reviewed with three reproduced
P2 findings. A held request can pass its decision to a new answered or
unjudged request without an intervening Resting state; `voice/turn` is
absent from the export topic census; and the second route can publish its
verdict before its queued transcript reaches the coordinator and Policy.
The [first two findings](https://github.com/autonomous-ai/lampOS/pull/1782#issuecomment-6261823722)
and [producer-ordering finding](https://github.com/autonomous-ai/lampOS/pull/1782#issuecomment-6261862180)
give the source paths, reachable sequences and exact commands.
Focused review evidence, kept separate from implementation gates:

- New-request regressions: **2 passed, 2 failed, 0 ignored, 173 filtered**;
  the same sequences with the feature off are the passing controls.
- Existing export census: **0 passed, 1 failed, 0 ignored, 321 filtered**,
  failing on `voice/turn`.
- Actual second-route Engine, with a callback barrier and no sleeps:
  **1 passed, 0 failed, 0 ignored, 2,052 filtered**; the verdict is published
  while the matching transcript is still queued.
- Policy ordering pair: **1 passed, 1 failed, 0 ignored, 177 filtered**;
  heard-before-verdict passes, the reachable reverse order loses the hold.

The additional regressions live only in a source-only review copy; their
[test-only patch and counts](verification/hello-quiet-peer-review-20261019/)
are kept with this log so the author can reproduce them. The reviewed
checkout and the task branch are unchanged. Native ARM64
[39213394277](https://github.com/autonomous-ai/lampOS/actions/runs/39213394277)
then confirmed the export failure: **6,799/7,322 tests run: 6,798 passed
(1 leaky), 1 failed, 48 skipped**, stopping on `voice/turn` in the census.
The corrected head `d923d46cf3b13bd14860ad1d048e0e9de31a4f7a` addresses
all three findings. A read-only source review found no remaining actionable
defect in those paths. The original independent checks now pass: Policy
**6/0/0, 176 filtered**; producer characterization **1/0/0, 2,053 filtered**;
export census **1/0/0, 321 filtered**. The [fixed-head patch and exact results](verification/hello-quiet-peer-review-20261019/)
are recorded separately from the original failures. Only test modules in
a fresh source-only review copy changed; the author's branch and clean
review checkout stayed unchanged. Native ARM64
[39217912208](https://github.com/autonomous-ai/lampOS/actions/runs/39217912208), linked to the corrected head,
then passed **7,376 passed (1 leaky), 48 skipped**, plus **4 doc tests** and
**12 own-harness tests**. The separate relay-signals harness, release build,
checksums and artifact upload passed. The existing same-voice leak and
example-output warnings remain in the record. Native log SHA-256:
`0357695e2c24e133cad0103c06f95ce69956d332ec286fbb0b7179513d20c52e`.
Peer verification of that corrected tree is complete. The replacement
[#1817](https://github.com/autonomous-ai/lampOS/pull/1817), head
`48edcb04f147b1ebe33b855fbdca7f2cfb13e374`, has the identical full Git tree
as reviewed `d923d46cf3b13bd14860ad1d048e0e9de31a4f7a`:
`3c72796463c28af2282f7d8e2e28b5072924d991`. It replaces #1782; the source
review and regressions apply to that unchanged tree. Replacement native
ARM64 [39220766616](https://github.com/autonomous-ai/lampOS/actions/runs/39220766616)
passed **7,389 tests (1 leaky), 48 skipped**, plus **4 doc tests** and
**12 own-harness tests**; relay signals, release build, checksums and
artifact upload passed. The [replacement native record](verification/hello-quiet-peer-review-20261019/replacement-head-arm64.json)
distinguishes its linked PR head from the tested merge checkout
`1994cad1d330635a7558b839fa859624a925916d`, whose base was
`da0806945ec6319fc1ce13443cd46945d317bca5`. The preceding 7,376 result
remains evidence for its own run, not this replacement checkout.
First merged #1817 at **08:32:01 UTC** as
[`9df25da794d1b7ab7bcc967aa020a63be1108b5a`](https://github.com/autonomous-ai/lampOS/commit/9df25da794d1b7ab7bcc967aa020a63be1108b5a).
No third implementation scope or device work was taken.

The new Queue checkpoint rule was checked against every open assigned issue.
Fresh checkpoints on [#939](https://github.com/autonomous-ai/lampOS/issues/939#issuecomment-6262420448)
and [#737](https://github.com/autonomous-ai/lampOS/issues/737#issuecomment-6262421179)
separate delivered work from remaining owner actions. #939 is closure
bookkeeping only, as First already confirmed; #737 has a successful local
signed-channel update/rollback proof, with published releases, production
signing and attended body coverage remaining. The existing dispositions on
#871, #800 and #694 already state the remaining work and done conditions.
#1123 reaches the age threshold only after 10:19:48 UTC. First retains all
closure and follow-up ownership.

## Completed assigned work: 2026-10-19 reconciliation

Ready for lamp-voice: yes (the replacements below are merged)

The original three forward-reader PRs closed **unmerged** after replacement
work landed; do not treat the closed original heads as the merged revisions.

| Assignment | Original PR | Landed replacement | Evidence recorded by the replacement |
|---|---|---|---|
| Delivery ledger reads forward | [#1559](https://github.com/autonomous-ai/lampOS/pull/1559) | [#1644](https://github.com/autonomous-ai/lampOS/pull/1644), `7004a106b` | Schedule **189/0/0**; [ARM64 39163484997](https://github.com/autonomous-ai/lampOS/actions/runs/39163484997) green. Additive persisted fields are tolerated; unknown enum values and live Ack/Guard fields remain refused. |
| Schedule store reads forward | [#1562](https://github.com/autonomous-ai/lampOS/pull/1562) | [#1616](https://github.com/autonomous-ai/lampOS/pull/1616), `7945fee54` | Schedule **181/0/0**; [ARM64 39137252268](https://github.com/autonomous-ai/lampOS/actions/runs/39137252268) green. Setting aside a newer schema preserves readable store identity and advances revision/IDs, avoiding reuse of archived reminder IDs. |
| Records reports read forward | [#1557](https://github.com/autonomous-ai/lampOS/pull/1557) | [#1643](https://github.com/autonomous-ai/lampOS/pull/1643), `6071c7520` | Records **168/0/1**, Settings **45/0/1**; [ARM64 39164368636](https://github.com/autonomous-ai/lampOS/actions/runs/39164368636) green. Unknown values in a supported schema remain refused; a newer report is archived and fresh cleanup evidence requires actual cleanup. |

[#1710](https://github.com/autonomous-ai/lampOS/pull/1710) merged as
`34405c6bfeaf4c95feb54573ea76533b47536044`. Its
[host evidence](https://github.com/autonomous-ai/lampOS/blob/34405c6bfeaf4c95feb54573ea76533b47536044/lampOS/docs/verification/holding-answer-review-host-20261019/README.md)
records **4,094 passed, 1 failed, 20 ignored**, not an all-green host result.
The unchanged `relay_clock` failure was traced to the macOS shell/FIFO signal
interaction and retained in the evidence; the later test-only correction
landed through #1743. Native ARM64
[39198152207](https://github.com/autonomous-ai/lampOS/actions/runs/39198152207)
passed **7,235 tests (1 leaky), 48 skipped**, plus **12 own-harness checks**;
the affected relay test passed in 0.035 seconds. `LAMP_HOLDING_ANSWER` remains
off by default.

The old open issues #1123, #939, #871, #800, #737 and #694 were audited:
their remaining work is lamp qualification, closure/disposition or an
explicitly deferred model decision, not an unstarted host implementation.
Their stale `status:taken` labels were removed without changing ownership
or closing the issues. The
[queue audit](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6260171715)
records the remaining owner actions.

## Taken

| # | Task | Branch | Taken | State |
|---|---|---|---|---|
| 1 | The end-of-turn model in a tenth of a second | `second/turn-end-fast` | 2026-10-14 | on `lamp-voice`; on the lamp 148 ms against 382; the voice does not use it yet |
| 2 | Try to break last night's changes to the voice | `second/voice-review` | 2026-10-14 | on its own branch, 2026-10-14 (`1d737f8e5`); taken into `lamp-voice` with its mends |
| 3 | A listener for "stop" under the lamp's own voice | `second/stop-word` | 2026-10-14 | on `lamp-voice`: hears the word alone; under the lamp's voice only through the canceller, partly; a first look on lamp-4ace heard no played stop; not wired |
| 5 | The name, heard as a bare "Lamp" (issue #694) | `second/bare-name`, then `second/bare-name-2` | 2026-10-14 | #725's model on `lamp-voice`; on lamp-4ace at 23:56 it heard 1 of 6 played calls against the installed model's 3 to 4: not installed; the second (#750) checked on lamp-4ace at 08:55: 1 of 6 at 0.9 against the installed 2 of 6, bare "Lamp" lifted but "Sam" and "Lan" as much: not installed, #750 closed; the 14 cut `say` lines asked for as a held-out lamp test; after the MVP |
| 9 | Faster answers: what exists | `second/faster-answers` | 2026-10-14 | on `lamp-voice` (a study, done beside 5 and 10) |
| 10 | A reference that follows the microphone's clock | `second/reference-clock` | 2026-10-14 | on `lamp-voice`, behind `--echo-reference steady`; on the lamp's recordings, mixed (two up, two down, one held) |
| 20 | The replay takes the lamp's own recordings | `second/replay-capture-clocked` | 2026-10-14 | on `lamp-voice`: five of the lamp's recordings followed sample for sample |
| 19 | The end sent early, the answer played only when sure | `second/early-end` | 2026-10-14 | on `lamp-voice`, behind `--early-end-ms`; played on lamp-4ace at 200 ms: worse; switched off |
| 36 | Always alive: the idle layer, sleep and waking (issue #714) | `second/always-alive` | 2026-10-14 | on `lamp-voice` (#729, merged 23:03); on lamp-4ace since 23:07 |
| second-P1 | The early end taken back (issue #723) | `second/early-end-back`, then `second/early-end-labelled` | 2026-10-14 | step 1 on `lamp-voice` (#731, merged 23:06); on lamp-4ace at 23:10 (a) did not hold and (b) did; step 2 as pull request #738 into `lamp-voice`, 2026-10-15 |
| 45 | Updates: a lamp keeps its lampOS current by itself (issue #737, MVP) | `second/updates` | 2026-10-15 | on `lamp-voice` (#741, `78b984b69`); lamp-4ace updated itself to build 188 from a signed test channel at 03:48; the publishing proposal agreed on #737; the CI `publish:` job not added by this session (it handles the signing secret and makes releases; this session's safety check refused the edit); left to the first builder or the owner; #1124 (2026-10-16): `lamp-update`'s lookup was counted out only after its answer was handed over, which turned #1003's and #1092's ARM64 runs red once each; now counted out first (`lamp-update` 77 passed, ARM64 green, ready) |
| 44 | A yes said in two breaths still starts the task (issue #736, MVP) | `second/two-breath-yes` | 2026-10-15 | #740 closed as not needed, 2026-10-15: `d128c458a` (the model told why a call was not done) started five of five on lamp-4ace; the branch kept at `0ebd994d5`; the news crash found in its minute fixed in #756 |
| 40 | Progress news through the tasks part and Jev (issue #732, `second-P2`) | `second/progress-news`, then `second/progress-reads` | 2026-10-15 | #751 merged by the reviewer at 17:47 (`ecca16554`), after four rounds of review (stale answers, the quiet from `resting`, a run identity across restarts, a new run's first count and the cooldown across the tasks part going); the first builder's asks as #793 (a value at start, a read at once, `unread` for a task not read yet, a journal line a read) merged by the first builder at 19:42 (ARM64 green, 4,501 passed); #732 open for the follow-up |
| 41 | Codex and the others: the person names who gets the work (issue #733, `second-P3`) | `second/who-gets-the-work` | 2026-10-15 | moved to the fourth builder at 07:07; the branch at `ec2fb2a4d` on `2176cd38a`, gated, with the tests the schedule tools needed; its state said on #733 and the claim released |
| 48 | The Policy decides when the lamp speaks first: progress news, then hello (issue #752) | `second/policy-speaks`, then `second/hello-asks` | 2026-10-15 | #765 merged by the reviewer at 18:14 (`4ad0ec5c3`), #752 closed; hello asking the policy rebuilt as one commit on `lamp-voice` `a583fda28` (`0aac359c1`, carrying the keeper's pause), gated, and proposed as #856 (`second-P5`) once #757 closed |
| 52 | Task news, polish: a follow-up question is not told the news again, and a long result gets a gist (issue #757) | `second/news-polish` | 2026-10-15 | #794 merged by the first builder at 20:41 (ARM64 green, 4,587 passed); #757 closed; the first builder listens on lamp-4ace tonight |
| 59 | A turn with a function call speaks 1.8 to 2.9 s after the person stops (issue #799) | `second/tool-turn-speed` | 2026-10-15 | pull request #810: `provider-check --tool-turns`, `turn-times`, call times in the first-output record, and the lead-in behind `LAMP_TOOL_LEAD_IN=on`; measured by the first builder with `turn-times` (plain 1.38 s, with a call 2.25 s, the second generation about 1 s); the reviewer's four findings fixed (`6cc4f2c09`); its fifth (audio between the call and our answer taken for the answer's delay) fixed (`1d06e35a0`); merged by the first builder at 19:42, ARM64 green (4,572 passed); #799 open for the lead-in's lamp check; the first builder's lamp check of 16 Oct 00:12: with the lead-in on, nothing called; found at 11:5x: `start_agent_task` was 1,036 bytes with two of the staff and the link takes 1,024, and `thread::equipped` then gave the session no function at all, silently; draft pull request #1020 (`second/lead-in`, `ac461cfe8`): the lead-in is the read-back as a statement then the call (1,005 bytes at most), a function the link would refuse is left out and named in the journal (the first builder's 11:57), every combination tested; `lamp-voice` 1,519 passed, 11 fixture spawn timeouts of the #887 family that passed alone; the first builder asked at 11:57 that a refused function be left out alone and named, done; the executive's refused `tools.json` said in the journal with why (`3c4c19c8a`, `lamp-voice` 1,530 passed); ARM64 green (5,432), ready at 13:0x; #1020 merged in the afternoon; Sixth's P3 there (a `tools.json` of 33 or more valid functions refused whole, where before the first that fit were told) as `second/catalog-room`: only what the link refuses refuses the file; #1085 (`3d1138e95`, `lamp-voice` 1,595 passed, ARM64 green 5,613, ready) |
| 60 | A short room sound heard as "¿Qué?" gets an answer (issue #800) | `second/short-sound` | 2026-10-15 | pull request #818: `short-turns` counts them from the records (kinds, no words), and `LAMP_SHORT_SOUNDS=quiet-room-checked` holds a let-through sound's answer unless it was a short word, said close, or someone faced the lamp; the count on today's lamp records asked for on #800; ARM64 green, marked ready; rebased twice (onto #778's second voice, whose worker now also takes `--sounds-checked`); the reviewer's 20:11 finding fixed: the second voice holds a short sound not heard as a short word (`3076f8452`, `lamp-voice` 1,326 passed); the lamp minute is the first builder's |
| 61 | Asked to have Claude Code do something, the lamp sometimes does it itself (issue #854) | `second/hand-over` | 2026-10-15 | draft pull request #859 (`9169de55e`), off unless `LAMP_TASKS_ASKED=on`: `start_agent_task` called whenever the staff are asked for anything, a story included, even mid-story; the session told so after the persona (both routes); `who` may be "not sure", which asks "Claude Code or Codex?"; `provider-check --hand-overs` counts it per scene; `lamp-voice` 1,334 passed; ARM64 green (4,595 passed). Played on lamp-4ace at 21:22–21:33: scenes 1–4, 6 and 7 as meant, and the switch kept on there from 21:30; the unclear name was guessed and asked about twice, and "not sure" never taken, so `afe87d6d9` names the guess in the read-back, asked once; the check's `asked_again` measured a yes and is now `after_own_story`; played again at 21:58: one question naming who, nothing started without a yes; #859 merged by the reviewer at 22:44 (ARM64 green, 4,686 passed), #854 closed; lamp-4ace keeps `LAMP_TASKS_ASKED=on`; reopened 2026-10-16 02:08: some sessions offered every story asked of the lamp to Claude Code (my wording had no limit on who was asked): pull request #903 (`second/story-told`), the words both ways and `start_agent_task` taking `named` and `on_the_computer` (nothing prepared with neither); its first head told the stories 4 of 4 on lamp-4ace at 02:43 but lost both named poems ("I can't write code …"), so the hand-over is said first and in full again and the exception after it (`b1f59b2b9`, `lamp-voice` 1,371 passed, one timed test passed alone 3 of 3); its second head (two required arguments) lost them too, 0 of 3, twice claiming a hand-over with no call (03:10); the third, `3c6775bb7`, leaves `start_agent_task` as it was and changes only the session note: `handovers.sh` on lamp-4ace at 03:37, told 4 of 4 and Claude Code named 2 of 2 with the read-back; ARM64 green, ready, on lamp-4ace; reruns at 08:00 and 11:00; #903 merged by the reviewer at 05:15 (head `7eb98c818`), #854 open for the 08:00 and 11:00 reruns |
| 62 | Jev 1.14: replay the Policy's logged decisions through 1.13 and 1.14, and pictures with the questions (issue #871) | `second/jev-replay` | 2026-10-15 | draft #873 (`cbbd8c58f`, `lamp-policy` 179 passed): `lamp-policy-replay` asks each logged attention question again of each model (no words out) and counts against labels; running on lamp-4ace since 23:18 on 3,054 decisions, 193 labelled, summary at 08:00; `lamp-policy --images` asks again with the eyes' frame (the voice's rule, 640×480, TypeSafe's `assets`) and five picture questions, in shadow, counted by `--pictures`; whether OpenRouter passes pictures on is the lamp's to show; the reviewer's four findings fixed in two rounds (`9e83243fb`: nothing written once the eyes' frame is gone, each label given once to its nearest line; `4f6c4fe5b`: the picture taken at the moment its request is written, contradicting labels at one moment refused), `lamp-policy` 183 passed, ARM64 green (4,770 passed); ready; the reviewer's 01:13 P3 (a picture written whole on a connection closed unanswered, then a try withdrawn, recorded as not sent) fixed in `679535244`: each try's delivery told, `sent` if any went, both askings counted on the same lines; `a6bb08751` adds `--again --refit` (each model's weights fitted again to its own answers on half the labels, counted on the other half, 200 times), for the switch; `lamp-policy` 192 passed; a refit that cannot be made said so (`45b3ca7dd`); rebased for #881's relay fix (`49c493727`); the picture asked first whether anyone is in it, for the empty desk's near-chance answers on lamp-4ace (`6702173f2`); `--cost-ratio N` (`c4785b870`, `lamp-policy` 195 passed) for the first builder's choice between 1.13 and 1.14 (stays 1.13 since 01:30); with #883 as `second/pictures-hello` (`804651c6c`), on lamp-4ace since 02:27, OpenRouter passing the picture; the reviewer's 04:14 finding (a picture asked from a queue after the face had gone) fixed with one shared `Presence` checked at dispatch and right before the frame (`e20350516`, `lamp-policy` 196 passed, ARM64 green, 4,968 passed); no finding left at 05:09, waiting on the first builder's camera check and #871's scenes with the owner; rebased over #943 (`bc8a03d77`, `lamp-policy` 200 passed); ARM64 green on `bc8a03d77` (5,313 passed); rebased over #937's `--rules` at 11:3x (`86cbdb078`, `--rules` beside `--images`, `lamp-policy` 205 passed); #883 merged minutes later, so rebased again: `policy::Beside` carries progress, hello and pictures beside the attention question, and `lamp-policy` takes `--hello` beside `--images`; the reviewer's fresh gate at 12:25: two real-time bus tests in `tests/policy.rs` failed on their host; measured here: 40 of 40 alone at head and on `lamp-voice`, idle and under load, and the whole file 95/95 at head against 94/95 on `lamp-voice` (a third timing test): the file's own flakiness, offered as an issue; rebased onto #844's `--zone` at 14:2x (`84484c64c`, `lamp-policy` 221 passed); #1060 filed for the Policy's bus tests and fixed by #1065 (`second/policy-bus-tests`: a 20 s wait, a diagnostic and a pause; with 0.4 s stalls every second `lamp-voice` failed 7 of 20 and the branch passed 40 of 40; ARM64 green, 5,526 passed; ready at 15:2x); rebased onto #958 at 15:3x (`0e45ea022`, `--gate` beside `--images`, `lamp-policy` 227 passed); #873 merged at 15:5x; #1065 merged at 16:08, #1060 closed; #1040: the shadow judgement proposed from the first builder's 79 rows (in conversation `3 × max(0, follows_lamp − 0.55)`: 13 of 33 said to the lamp leave `stay_quiet`, 0 of 46 not; a fit with each row left out gave +3.02 at 0.55) and built as draft #1106 (`lamp-policy --attention-next`, `next` on each line, `attention_next_differs`, nothing published; `lamp-policy` 231 passed); #1106 merged at 18:2x; the reviewer's two P3s in #1142 (the fit is a ridge fit, penalty 0.5, and a plain fit gives +5.02 at 0.51 as the reviewer found; the switch-off run compared directly; `lamp-policy` 232 passed); the fit script on #1040; #1142 merged at 20:5x |
| 63 | The Policy holds the lamp's actions (issue #800): the "Thailand" `volume_up` of 15 Oct 14:49 on a `stay_quiet` | `second/hold-actions`, `second/act-attention` | 2026-10-16 | the voice behind `LAMP_POLICY_HOLDS_ACTIONS=on` (`23c26d9bf`): a volume or light call waits up to 2 s for the turn's attention decision made for real, and on `stay_quiet` without the name is not run and the turn's sound is not let out; the Policy's `--act attention` (`f8a17a0ad`, on #873 and #883 as `second/act-attention`, its pull request once they merge); pull request #908 ready (ARM64 green, 4,895 passed); played on lamp-4ace at 03:28: "someone should turn it up a little" in a busy room, `stay_quiet`, not done, 558 ms, nothing said, the volume kept at 75%; the reviewer's 03:45 and 03:48 (pairing, cancellation) fixed in `1502a26c6`, then at 05:09 and 05:11 two more: a late decision still taken for another turn, and a cancellation stuck behind full output. Fixed in `6c1dc01fa`: `voice/heard` carries the turn whose input began last, the Policy's `Decision.request` already carries it on, and decisions are paired by that number; the session notes a withdrawn call as it reads it (`call_withdrawn`), asked right before a kept call runs. The reviewer's 05:52 P3s and the first builder's 05:57 in `1ab26f986`: only `set_timer`, `set_reminder` and `cancel_reminder` of the schedule held, and the cancelled-turn test settled after the real stop with the worker healthy; `lamp-voice` 1,406 passed; pushed at 06:1x for `holds.sh` on lamp-4ace; the reviewer's 06:40 P2 (one Policy utterance across two turns, labelled the later) fixed in `27d969e05`: a piece of another turn ends the open utterance; `holds.sh` on lamp-4ace at 06:51 on `1ab26f986` (the two people's lines and the overheard "brighter" not done, named actions run in 0 to 2 ms); ARM64 green on `27d969e05` (5,058 passed); `second/lamp-holds` (`cba140ea7`) is lamp-4ace's build of it with `--act attention` and #883; for the first builder to merge; #908 merged by the first builder at 07:41; the Policy's side as pull request #943 (`second/attention-acts`, on today's `lamp-voice`, `lamp-policy` 166 passed, ARM64 green, 5,201 passed), the reviewer's only finding a doc line (a named turn's call runs at once), fixed in `c73293fd7`; #943 merged by the reviewer at 09:04 as `fca65fde5` |
| 64 | A person's kept rules about speaking first (#813, with the seventh) | `second/policy-rules` | 2026-10-16 | the interface agreed with the seventh at 02:41 (`memory/rules` gains `hello` and `news`); `lamp-policy --rules` for `quiet` and `quiet_mornings` built (`31fd05ae1`, `lamp-policy` 206 passed), held until #873 and #883 merge; the windows and "while talking" when the seventh's fields land; rebuilt on `second/pictures-hello` with the seventh's #904 limits (`5fdb9e1e0`: per kind `never`, `not_before_minute`, `not_from_minute`, `not_while_talking`, the last held while the room is not known quiet; `lamp-policy` 214 passed); its progress half on today's `lamp-voice` as pull request #937 (`second/rules-news`, `1e7f9e19a`, `lamp-policy` 167 passed) so that #813 need not wait for #873 and #883; the hello's half follows #883; #937's room checked when the answer comes, not when it was asked (the reviewer's 08:06), fixed in `b5185b121` (`lamp-policy` 168 passed, ARM64 green, 5,205 passed); the seventh closed #813's own side at 08:15; rebased over #943 (`cdc6ee1ba`, `lamp-policy` 171 passed, ARM64 green, 5,256 passed); no finding left at 08:50, waiting on the spoken keep/forget check; #937 merged in the 10:33 round; the hello's half (`second/hello-rules`, on #958): with `--rules` the hello question holds on `hello` limits, the moment question `news` on `news` and anything else on both, guard `kept_rule`, no model asked; `--rules` needs `--progress` or `--hello`; draft pull request #1003 on #958 (`994c7b932`, `lamp-policy` 191 passed); draft #1003 (`2775d4994`, rebased on #958's fix, `lamp-policy` 194 passed); #1040 filed for the attention weights (`follows_lamp` −0.136 though it tells the lamp's lines apart, median 0.60 against 0.29 on the first builder's 79); the guard #1054: a `stay_quiet` that follows the lamp at 0.6 or more keeps its timers and reminders (17 of 33 against 6 of 46), `lamp-voice` 1,574 passed; #958 merged, so #1003 retargeted to `lamp-voice` (one commit); #1003 rebased over #873 (`13e2cbf13`, `lamp-policy` 231 passed, ARM64 green 5,617, ready); peer review of Fourth's #1090 at the first builder's 16:11: two P2s (a capitalised opener before a comma, "Tomorrow, remind me…", now holds the line itself, shown by a probe on the head and the merge base; one attention test fails 3 of 3), and #1054 needs one more voice-side rule after it (a line after someone else's unanswered question in a quiet room); #1082 merged at 16:16, so #1003 rebased over it (`cb6dd64ff`: the kept rules' `hold()` and #1082's `busy()` both kept, the rule applied whether or not an answer came, both sides' tests kept whole; `lamp-policy` 233 passed; ARM64 green on the re-run); the reviewer's 18:22 on #1003: the P2 (a kept rule held a reminder) and three P3s fixed in `cedbffb1b` (rebased over #1099 and #1106; `lamp-policy` 238 passed); `never` across kinds left for the owner; peer re-check of Fourth's #1090 at `825f197ee`: no finding (`lamp-voice` 1,607, one real-clock test of mine flaky 1 of 3 alone, reported on #823); #1054's follow-up (a turn after someone's unanswered line is held as any `stay_quiet`) built on #1090 and gated (`lamp-voice` 1,612 passed), pushed when #1090 merges; rebased after #1090 merged (`2594772ee`, ARM64 5,710), ready; Fourth's peer review (P2: a cancel on a reply to the lamp is silent and cannot be undone; two P3s) fixed in `e3c566f7e`: only `set_timer` and `set_reminder` run on a following `stay_quiet` (`policy_holds::SETS`), a short sound is nobody's line, the doc slips fixed; the four changed tests fail on the old rules; `lamp-voice` 1,626 passed, ARM64 green (5,750); the reviewer's P2: the 11:20 timer scored `follows_lamp` 0.35 and stays held at 0.6, now said plainly; and a reply to the lamp only in conversation (the lamp's sound within 20 s, the Policy's own window), in `acce50bcb`, rebased onto `36b872d1c` (one test-helper conflict) as `5a253519e`, `lamp-voice` 1,692 passed; Fourth's re-check clean; the reviewer's re-check at 02:45 clean (checked to merge with #1131's clock change); merged at 03:0x; #1003 merged at 23:02, the reviewer's two P3s on it folded into #1143, rebased onto `lamp-voice` (`bd655b589`, `lamp-policy` 239 passed), retargeted; the seventh's peer review (no P1 or P2; P3: the old reminder branch could no longer run) answered with a `debug_assert_ne!` in its place, rebased onto `lamp-voice` (`acf244f73`, `lamp-policy` 239 passed); the reviewer's P3 (the test showed only the news out) fixed in `446c39dc6`; its ARM64 run failed on an Expression test this PR doesn't touch (`sleep_name::a_clock_not_set_…`, reported on #823), rerun green; merged at 03:0x |
| 65 | The progress question counts a slow answer as unasked, and a turn that speaks twice twice (from #751, found on #883) | `second/progress-turns` | 2026-10-16 | the same rule as the hello's: a turn's kind known at its start, counted once at its first sound (`f120bda7f`, on `lamp-voice` `28d581215`); pull request #932, ARM64 green (5,002 passed), merged by the reviewer at 07:10 as `e5aa43747` |
| 66 | Physical skills: the say service, the Policy as the gate, Reminders and Task news as skills (issue #939) | `second/say`, `second/gate` | 2026-10-16 | the say service as pull request #954 (`LAMP_SAY=on`: `{skill, note | text, priority, expires_in_ms}` on `/run/lampos-say/say.sock`, group `LAMP_SAY_GROUP` (`orangepi` on lamp-4ace), answered `begun` then `spoken`, `dropped`, `expired` or `refused`; a reminder's gates and its proof of a whole turn; `lamp-voice` 1,450 passed, ARM64 green, 5,209 passed); the Policy's gate on the bus as pull request #958 (`--gate`, `skills/ask` answered by a `Decision` naming the ask; `hello` through #883's question, `news` and `unprompted` a rule `wait` until their `moment` question; stacked on #883, `lamp-policy` 176 passed); Reminders and Task news as skills wait on the seventh's answer on #939 (their words come with the trigger, which `do.say` cannot carry yet); #954's reviewer findings fixed: one deadline for the whole line (`719b1850f`), and the expiry counted from the line and looked at again as it begins, the line at most 4,096 bytes with its newline (`cebb2eba4`, `lamp-voice` 1,454 passed); #958 rebased with #883 (`ff38b8a7d`), #959 a draft only for its ARM64 run; #954 accepted on lamp-4ace by the first builder at 10:17 (`begun` 1.09 s, `spoken` 4.64 s as `orangepi`) and merged in the 10:33 round, as was #937; that round left `lamp-voice` unbuildable (#954's `Reply.say` and #926's `cut_in` against #830's offline turns), mended by pull request #976 (`second/fix-voice-build`, merged at 10:4x as `f5a20ba28`; voice 1,510 passed, one timed test passed alone 3 of 3); next the `moment` question for `news` and `unprompted` asks, owned by the hello question (`second/moment`); the moment question as #958's second commit (`a80e52ccb`: never while anyone talks to the lamp, then `good_moment` on its own connection, a rule after 1.5 s, let go after 2 s as `late`; `lamp-policy` 187 passed); #883 merged at 11:25, so #958 is rebased onto `lamp-voice` (`c02555c7f`, no conflict) and retargeted there, and #959 closed; ARM64 green on `a80e52ccb` (5,390 passed); #958 ready at 12:0x (ARM64 green, 5,394 passed); for Reminders and Task news, (c) proposed at 12:16 instead of (a): the skill triggers and moves, its `say = { from = "schedule" | "tasks" }` asks the voice, which keeps the words and the durable delivery and says a reminder itself if no skill asks within 10 s; the reviewer's two P2s of 12:19 on #958 (a side queue that could lose an ask's name; the 2 s checked only before asking) fixed in `2c890f074` (each ask carries its name and receipt; the model gets what is left; a later answer is let go as late; `lamp-policy` 190 passed; ARM64 green, 5,436); (c) agreed at 12:32; Task news moved to Jarvis (#1021); `say = { from }` in `crates/skills` as #1033 (`lamp-skills` 23 passed); the voice's side as #1038 (`from` in the say service, `LAMP_SCHEDULE_SPEAK=skill` and `LAMP_TASK_NEWS=skill` waiting up to 10 s for the skill; `lamp-voice` 1,557 passed); the Reminders folder as #1039 on #1033 (three recorded cases); #1033 and #1038 ready (ARM64 5,442 and 5,465); Jarvis's Task news needed the end of a task on `tasks/status`, which the watch let go before `ps` showed it idle: `settled` kept two minutes, #1048 (ARM64 5,464, ready); #958 rebased onto #844's `--zone` (`692702dc1`, the moment question on the same `LocalTime`, `lamp-policy` 193 passed), #1003 with it (`e15b0dc36`); peer review of Twelve's #1009 at the first builder's 14:05: no finding; #1038's three P2s from Fourth fixed (`d780d1443`: a `from` ask taken with the switch off and living 20 s; while a reminder is held for its skill the say service, the hello and the news wait; `lamp-voice` 1,600 passed; ARM64 green, 5,544 passed); #1033's P2 and P3s fixed (`8f9b1fe2c`: `say.from` in a skill with a tool refused, `say` read by hand with plain errors; `lamp-skills` 31 passed); #1039 rebased onto it (`7bff9c23d`); #958 merged; its two P3s (an asker gone while someone talks; `busy` without the moment question) as `second/moment-held`; #1048: Fourth's P1 closed by Jarvis's #1042 (`appears` on new items), the P2 runtime test and the P3 (only the watch's own latest read counts) in `4c8058112`; peer re-check of Sixth's #1049: no finding; #1033, #1038 merged at 15:57; #1082 ready (ARM64 5,608); #1039 retargeted and ready (ARM64 5,613); Thirteen's two P3s on #1038 answered in #1087 (docs only, ready): an ask names its source, not an item, and the loop's guards are pinned by no test, as no harness drives `run_inner` (a day's work, not taken); #1099: a skill's reminder asks the gate as `reminder`, held only while someone talks (`lamp-msgs`, `lamp-skills`, `lamp-policy` together; 105, 31 and 228 passed); #1087 merged; #1099 rebased over #1082 (`84da46b2b`, 105/230/32 passed, ARM64 green, ready); the reviewer's P2 on #1039 is #1099's fix, so #1039 merges after it; #1099 merged at 18:1x, its three P3s in #1143 (stacked on #1003: a waiting reminder decided at once even behind a question out; `lamp-policy` 239 passed); #1039 rebased onto it (`7fc1f1d77`, `lamp-skills` 33 passed); #1124 closed as the duplicate of #1118; #1048 merged at 20:56 (Fourth's re-check clean; `get_progress` answers a settled task's end, Ten and Nine's problem 3 (b), shown on lamp-75c9 by Ten; a settled task timed from its first-kept answer so #1145's follow looks never tell it twice, `d119c2ee3`); its doc P3 as #1150; peer reviews: the sixth's #1086 (three P3s, fixed, re-check clean, merged), Thirteen's #1134 (two P3s), the sixth's #1139 (two P3s: a call split across pieces leaves its tail, shown by a probe); the seventh's #1146, the voice part (two P3s: any tool's `"look": true` opens the camera, not only a skill's; the backup voice never takes the look); Fourth's #1107 (two P3s: an idle Stop opens the 4 s too; a real short word in them is lost in every mode, unsaid in the docs) and #1108 (one P3: three comments over the wrong code), named by the first builder at 21:25, each run merged with `lamp-voice` `f1e13fbe5` (#1108's P3 fixed in `13709c362`, re-check clean); the eighth's #1149 (two P3s: its ignore reason predates #1145's confirming look, so a faster look has 10 s, not 20: it passes with the watch at 5 s and fails at 15 s; only a faster look can pass it) and the sixth's #1154 (one P3: no test that a refused picture's retry stays inside the 5 s; the old 3 s bound fails its new test); #1054 ARM64 green (5,710) and ready |
| 67 | Short sounds with no words cut off answers still to come (#1123, the sixth's 16:5x, the first builder's 17:05) | `second/wait-keeps-answer` | 2026-10-16 | the decision is the coordinator's, before `commit_admitted_input`, for both voices; no words of a new sound can be had before it takes the turn, so `LAMP_WAIT_KEEPS_ANSWER=on` works on length and pauses: while an answer is awaited unheard, a new sound waits (pending, its audio kept), and 1 s after its trigger, or 100 ms before its own deadline, it is let go if quiet 300 ms, else takes the turn from its start; an ended one is let go at once (the admission would otherwise end the service on the next sound); three tests on the real admission, detector and mask with the duplex hold; the word-free count script posted on #1123 for lamp-4ace; #1130 ready (ARM64 green, 5,624); the reviewer (one P1, three P2s) and the first builder (three P2s): on the product's mask a sound that waited 1 s plus its pre-roll was rejected as evicted, and the tests' mask was switched off; rebased onto `f40a6907b` and fixed in `92d06aefa`: the mask keeps 192 blocks with the switch, a sound that began waiting keeps the rules once the answer sounds, only answers to someone and within 3 s, the name takes the turn, a sound within 1 s after one let go is taken, tests on the product's duplex mask with a full pre-roll (each new rule's test fails without it), `group-talk.md` moved and measured (0.75 s or less let go, 0.8 s or more taken); `lamp-voice` 1,664 passed, ARM64 green (5,779); the first builder's re-check (a new P2: once the answer plays the mask scores the person 0.0, so a waited follow-up looked quiet) fixed in `0caceba87`: a waited sound is judged by its pause as it stood when the answer began to sound, and a block without echo evidence does not wait; my first test of it fed "speaker quiet" evidence and could not fail, the control showed it; approved at 23:53, ARM64 green (5,876); merged at 01:4x |
| 68 | A result told within 20 s of the agent settling (#560, the first builder's bound at 20:49; Nine and the second to choose a faster look or a push) | `second/news-on-settled`, `second/news-at-pause` | 2026-10-16 | the proposal on #560 (22:00): Harness's own `wait --machine … --any` (looks every 3 s, idle on two looks at one turn; `--machine` came to `wait` with `show`'s, #1333), about 10 s, against about 19 s and a Node start every 5 s for `ps`; Nine took it in `lamp-tasks` (`serve --push-wait`), the eighth left the voice's gate to me; the voice's half: (1) #1156, it reads at once when `tasks/status` lists a task newly settled, not up to 10 s later (a watcher, `Wake::Settled`; the no-switch integration test now says what it listens to; `lamp-voice` 1,638 passed), (2) with `LAMP_TASK_NEWS_AT_PAUSE=on` news at the first pause of a second, not after 15 s with no turn (`quiet_for_news`, the eighth's case 19 s against 33 s), stacked on #1156 as #1161; the first builder's review of #1156 (P2: no `tasks/status` without `--progress`, which Nine adds to the units in #1157; five P3s) and the reviewer's two doc P3s fixed in `b37fbe7dc` (`start_with_clock`, no news lost if the watcher can't start, at most one woken read a second); Ten's peer review clean, with lamp-75c9's numbers: settled 8.3 to 10.2 s after Harness's idle with `--push-wait`, so the first word about 10 to 12 s; the disk monitor (dee's ask) had my three build folders at 190 G: two deleted and one cleared, about 167 G freed, now one folder, one build worktree, `CARGO_INCREMENTAL=0`; #1156 merged at 23:5x; #1161 retargeted, the first builder's P2 (1 s after the lamp's own words is the person's turn) fixed: after the lamp's words 3 s with nobody starting (`AFTER_LAMP_US`); a wrong claim of mine on #1156 (that #1157 put `--progress` in the units) corrected within minutes, the units come with Nine's #1174; #1161's fix gated (`f6d45dd04`, `lamp-voice` 1,640 passed), Twelve's peer review clean, ARM64 green (5,869), merged at 01:4x; Nine's #1174 (`--progress --push-wait` in the product unit) merged at 01:42, so #1156's wake now acts on product lamps; #1143 and #1054 ARM64 green, waiting on review; peer review of Thirteen's #1170 (one P2, taken: with a volatile journal a day's window dropped this boot's earlier runs as `other_boot`, shown by a probe; three P3s); the reviewer's after-merge P2 on #1161 (the lamp's own echo in the 0.27 s after its words, scored as speech, skipped the 3 s: told at 1.2 s) as #1217, ready at 05:32 with ARM64 green (6,179), merged by the reviewer at 05:33 (`LAMP_TAIL_US`, 300 ms, the duplex mask's own wait; the old line fails its new scene at 1.2 s); peer review of Fourth's #1197 (one P2, the reviewer's 04:50 reached separately: over the answer's first moments the evidence is `Unknown` and the rule hears none of the asker; a fixture probe with 0.15 s or more of that window lets the asker's follow-up go under `on`) |
| 69 | The voice ends at midnight on a lapsed authority (#1178, the first builder's 03:06) | `second/midnight-lapse` | 2026-10-17 | missed for 90 minutes (I read page 7 of #705 while its comments went to page 8), taken at 04:31; the speaker worker was the part that ended the voice: held up past a turn's 100 ms lease with nothing fresher queued, its `maintain`, `zero_authority` or `start_clock` returned `ExpiredState` and the worker ended; draft #1214: a lapse stops the device, reports the reset as after any discard, writes and primes nothing until a fresh authority, then primes the clock again (`Speaker::authority_lapsed`); Mac gate green on `183ec0a01` (`lamp-voice` 1,801 passed, `lamp-audio` 161; the speaker loop and its new worker test are Linux-only); ARM64 38971245058 failed in that test (the worker ended at once and the test did not say why), so the test now reports the worker's own error (`bd87afbad`, run 38973710923): `reference InvalidPrime … accepted=480`, ALSA's `null` plays every frame at once and the prime needs 480 queued, so the clock never starts there; the test now shows the lapse before a prime (the capture side ready only once the lease ran out): ARM64 green on `4df65b978` (6,182 passed), and a control without the fix, run by hand on a branch since deleted, fails on "authority snapshot has expired"; a lapse with sound queued and the prime after it are for a lamp; marked ready at 05:4x; Ten's peer review (no P1 or P2, run on Linux: 3 of 3, the control fails) and the reviewer's 05:57 (P2: a lapse while the lamp speaks still ended the voice, through the coordinator: `PlaybackDiscarded { ExpiredState }` taken as unexpected, and the pending chunk written after the lapse rejected; P3s: no upper bound, fold the fix-ups); fixed at `5b2a6d4be` on one folded `4f0663cf5`: the speaker reports the speech a lapse drops as the lapse (its own ownership check, then the pending chunk refused with `lapsed: true`), the coordinator cuts that answer as a reminder's lapsed lease does (`speaker_lapsed`) and goes on, a late prime from before a reset is dropped (Ten's P3 was a real way to end the worker, `InvalidStart`), and a lapse past 1.5 s ends the worker; `coordinator::tests::speaker_lapse` (3, all fail with the old handling) and a second worker test; Ten's re-check ran the late-prime control on Linux (fails without the drop) and the reviewer's found no P1 or P2 (P3: the cut is tested by direct calls, not through the loop); merged by the first builder at 06:42, onto lamp-4ace voice-only before tonight's midnight with `LAMP_STALLS=on`; the reviewer's note: a held speaker (over 20 ms, an ALSA underrun) still faults, as before; the reviewer's loop P3 as #1225 (test only: through `conversation_with_gate`, a lapse after the first answer's second chunk ends it `speaker_lapsed` and the next question is answered; with the cut taken out the session ends in an error; the provider queue's clearing is not what holds the rest back, said so), ready at 07:1x with ARM64 green (6,197), merged by the reviewer at 07:14 |
| 70 | Tasks that finished together told in one turn (the reviewer's P3 on #1161) | `second/news-together` | 2026-10-17 | taken at 05:50 with my list empty; #1223 (`b5ed7ea21`): with `LAMP_TASK_NEWS_AT_PAUSE=on` up to three finished tasks with an answer go in one note, one sentence each; a note holds 1,024 bytes, so fewer go when each answer would keep under 120 characters (three long ones go as two, about 210 characters each, then one); `lamp-voice` 1,811 passed, ARM64 green (6,189), marked ready at 06:15; Jarvis's #1042 (the Task news skill) waited on my voice side, which was in since #1038 (15:57 on 10-16): told on #1042, and #1223 tells one at a time while news waits for a skill (`ffe9021ef`, `lamp-voice` 1,811 passed); merged at 06:48 on the reviewer's clean check (the first builder's new rule: off by default, ARM64 green); its two P3s (a task left out went ahead of one waiting for the person; a cut answer told together lost #757's guard) came after the merge, as #1231 (`48df23ac5`, two long answers now keep 178 characters each, each fix with its own failing control); the owner's ask to bring PRs and issues down: closed #714, #799 and #1040, one line of what's left on #939, #1123, #800, #871, #737 and #694; #1231 no finding (07:43); the first builder's 07:31 build list put #939 with #1042 on Jarvis and me: Jarvis asked for the voice side of the reviewer's #1042 P2 (the run's moves went with another task's news), built as #1237: a `from: tasks` ask may name its task and is given only that task's news, and news is not told together while such an ask waits (the reviewer's #1223 P3), `lamp-voice` 1,834 passed, ARM64 green (6,254), ready at 08:1x; Jarvis's `task` follow-up opens when it merges; Ten's switch census #1238: my rows right, two of my flags (`--early-end-ms`, `--echo-reference`) added at my note; #1237 merged at 08:55; peer reviews: Third's #1244 (no P1 or P2) and, assigned and missed at first, Nine's #1242 (no finding; the reviewer then found a P2 I missed, the 25 s budget unpinned, fixed by Nine in #1256) |
| 71 | `LAMP_SCHEDULE_SPEAK=skill` never said a reminder (the reviewer's P1 on my #1038, first on my list at 08:28) | `second/schedule-skill-speaks` | 2026-10-17 | the worker's `--schedule-speak` came only for `on`; one rule now, `reminders::speaks`, for the parent and the worker; the reviewer's flag test and a worker test that a reminder under `skill` reaches the model, both failing with the old rule; #1253, gated after #1254 fixed lamp-voice's `lamp-body` tests, ARM64 green (6,272), Third's peer review clean, merged by the first builder at 09:50 |
| 72 | #828's listening gaps (the first builder's 08:50) | `second/listen-gaps` | 2026-10-17 | a probe through the real `EchoObserver` and `Hearing` showed a gap of zeros alone gives the yield nothing to hear (5 to 6 `Over`, then `Unknown`); proposed both parts on #828, the first builder agreed at 09:09; draft #1264: the gap (a sentence end past 2 s held to 250 ms, the speaker writing zeros, `listen_gap`) and hearing in the lamp's own silence (`EchoEvidence::silent_at_echo`, `Hearing::silence_heard`), off unless `LAMP_LISTEN_GAPS=on`; each end-to-end test fails with its piece out; the lamp check is the first builder's, both sides (self-yields with nobody speaking, and the stops); the pull request's ARM64 never started, so run by hand: green (6,281), ready at 10:0x, front of the reviewer's queue; the sixth on lamp-52e6 at 10:02 to 10:18: 0 self-yields in five stories with 23 gaps taken, and the cut-ins yielded 4 of 10 on against 5 of 10 off, so it is safe off by default but its gain is unproven at n=10 (52e6 had yielded 9 of 10 at 08:24, and the first builder suspects Gemini's own barrier for both lamps' drop); Nine approved at 11:0x with two P3s; rebased onto `944ac0916` (one `duplex_policy` line conflicted) and both taken at `928844f17`: the held path tested, and zeros that run on more than 100 ms past the hold's end counted as a delivery gap once they resume (from the hold's end, since late words are released late); Mac gate green (`lamp-voice` 1,864, `lamp-audio` 163); ARM64 green (39002928926); merged by the first builder at 11:53 (`ae2919aa6`), off unless `LAMP_LISTEN_GAPS=on` |
| 73 | #1123 step 2: a short sound while an answer waits, judged by its words (the first builder's 10:21) | `second/wait-words` | 2026-10-17 | `LAMP_WAIT_KEEPS_ANSWER=words` (needs the second voice's configuration): a waited sound's audio, from its pre-roll, to a speech-to-text stream of its own kept ready while the answer is awaited (`RelayEars::for_waits`); its words asked for after 100 ms of quiet; anything past fillers takes the turn at once, fillers or nothing leave it to `on`; an ended sound is never held (the next sound's start would end the conversation); the words may take 490 ms for a sound of 0.5 s or less and 300 ms at 0.75 s, measured through the real admission and detector, against the relay's 225 to 269 ms on lamp-4ace (asked at 300 ms of quiet they would have had 290 down to 90); the privacy line noted in `stop-word.md` (the lamp-4ace decision does not cover it); #1277 (`362cf80ab`): Mac gate green (`lamp-voice` 1,865), ARM64 green (39001684271), the Seventh's review at 11:23 (one P2: a laugh or a lone "Okay." counted as words and would cut off the answer; two P3s: the stream open for the whole wait, and a check that never matched): laughs (only h, a and e) and acknowledgements now ask nothing, and the stream is kept only within the 3 s a sound may wait (`words_awaited`), each failing with its fix out (`4692e83d9`, on `0dc0e0c14`); the Seventh's re-check clean at 12:14, its two last P3s answered in `group-talk.md` (`4719e2c19`: the loop's close untested, as the stop words' is; the lists English only); ARM64 green (39004666177); ready for the reviewer; not run on a lamp |
| 74 | lamp-cb94's voice ended at 04:53:53 UTC on "Connection refused" (#1178, the first builder's 12:30) | `second/refused-voice` | 2026-10-17 | nothing connects: the workers' datagram sockets refuse a send once their peer has closed; read from the code, the capture worker ended first on `ReferenceLate` (`--reference-recovery off`: a speaker that sends nothing for as long as its last reported queue holds ends it: on cb94 456 frames, 19 ms, the last reference 20.0 ms before the fault, per Thirteen's read of the session record), then the speaker's reference and the coordinator's `publish` to it were refused, and on Linux that refused send also drops the gone worker's unread fault (shown on ARM64), so the voice ended on the refusal; draft #1298 (`0e2b54741`): a refused send names its socket, and a worker test shows `off` ending and `bounded` recovery (which exists, reviewed, never on a lamp) carrying all 310 blocks through a held-up speaker; no unit changed, the switch the first builder's call after a lamp-4ace soak; Mac gate green (`lamp-voice` 1,878; the 25-crate clippy fails only on the base's `lamp-selftest`, which #1276 fixes); ARM64 39007537193 green, the worker test being realtime in full run 39009464164; the hold was the kernel's: the SD controller retuning with "wait dma hold bit clear timeout" in the same second (Thirteen, 12:47, which I missed for 90 minutes and then corrected the 40 ms in #1298, `afcc0add8`); Twelve approved #1298 at 14:19 and put it on lamp-cb94 with `bounded` at 07:21 UTC for an hour; lamp-4ace already ran `bounded` from a drop-in; after it, the first builder's 13:57: peer-reviewed the eighth's #1305 (the capture open retried on `ENODEV` at start: approve; the 10 s bound holds and nothing ages, since the speaker primes only after `CapturePrepared`; one P3, gone then busy giving up at once, taken in `03f9b0c59`) and Ten's #1306 (the stop-word counts keep the last 16 and the first 16 stops: approve, the old rule failing both new tests); #1298 merged by the reviewer at 14:53 (its 13:59 P2, the queue rule, already corrected), its two optional P3s in #1321 (the test's upper bound and two wordings, tests and docs only); peer-reviewed Third's #1310 (approve as a comment, the account being mine; both probes fail its test), merged; #1277's privacy line said exactly (`0b9b813b6`: any sound in the 3 s, asked or not, up to about 1.3 s, to the second voice's speech-to-text); also gave my owner's yes on Nine's #1263 (`Records::state` marks the keys at a set-aside) |
| 75 | `lamp-records` failed every run on lamp-4ace, so the voice's sessions were never trimmed (the first builder's 15:10; records are mine) | `second/records-gone` | 2026-10-17 | pid 2431819 live with an empty `cmdline` on the guard's one immediate second look (`after_failure`), beside `gone: 1, zombie: 2`: a process ending with more than one thread, its leader having let go of its memory, reads so, as does one mid-`execve`, and a moment later it is gone, a zombie or readable; the guard had no retry, so it refused; now a process still live and unreadable is looked at again up to three times 20 ms apart (`SETTLE_LOOKS`, `SETTLE_WAIT`): gone, zombie or exiting skipped, readable inspected as live and protected, still unreadable refuses as before; three tests through an injectable wait, the two later-look ones failing with one look; #1328 (`f42fc130d`), Mac gate green (records 152/0/1, clippy on 25 crates); for tonight's build if it gets a peer review; also peer-reviewed Third's #1325 (the setup light: before the marker only setup can open the ring, and it lets go before the marker; a marker-first mutation fails its test; approve as a comment), and #1321 merged at 15:17 |
| 76 | `lamp-records` on a product lamp, from Twelve's lamp-cb94 check (the first builder's 15:54), and #1277's P3 (16:44) | `second/records-gone`, `second/waited-stop` | 2026-10-17 | records refused every run on a product lamp (`main process disagrees with manager invocation mapping`: the unit names `voice/current/lamp-voice`, the process's `exe` is the build it points to): the unit's binary is now resolved through its links before comparing (`2dfa86830`, a test through a fake `current` link); and install placed no `/etc/lampOS/records.json`, so the timer ran nothing: install now places the product's (on, 2 GB, 14 days, as lamp-4ace; `09168e649`); both folded into #1328, merged by the reviewer at 16:24 with #1277 at 16:25; then #1352: with `LAMP_WAIT_KEEPS_ANSWER=words` a lone "Stop." said before the answer (`wait_words::only_a_stop`, by `stop_words::stop_phrase`, nothing more asked) is never asked of the model and stops the answer it waited for through the loop's spoken-stop path, a stop with more asked still takes the turn; three tests, all failing with the check out; #939 and #908 have nothing left for me to build (all merged; what remains is the lamp checks and, after Sunday, retiring the old reminder consumer and `task_news.rs`); and a correction owed: the reviewer's 12:37 P2 on #1277 had been there all along, my search missed it |
| 77 | #1352's reviews (the reviewer 17:24, Fourth 17:38, the Seventh 17:40), and #1359 (Tests, 17:55: my #1298 test's bounded half failing about 1 run in 6 to 10 on Linux) | `second/waited-stop`, `second/rig-reads-the-stop` | 2026-10-17 | #1352: a waited stop counts only if no speech came after its words were asked for (`asked_through_us` against the loop's `last_speech_us`), so "Wait, … what about tomorrow?" with a comma's pause takes the turn; both stops are taken every pass (`1c5850a1c`); once `on` has decided because the answer began to sound, a waited sound still quiet since its words were asked is judged by them (`then_once_decided`), so a "Stop." ending under 300 ms before the answer still stops it; a stop with the lamp's name or still going at its 1 s mark takes the turn as before, said in the docs (`2abd2103f`, rebased on `7f2ec478c`); each new test fails with its fix out; Mac gate green (`lamp-voice` 1,917/0/11); #1359: not the capture path but the rig: `finish` renewed authority before reading, and when a renewal fell due just after the worker had stopped and gone, Linux refused it and dropped the unread `CaptureStopped`; now nothing is sent after what ends the worker (`Rig::end_with`), with a test that makes the order certain; #1360 (`eb65081c5`), Linux only so not compiled on the Mac; ARM64 full runs on it and on a control branch (`second/rig-stop-control`, renewal back, expected to fail) |
| 78 | #1352's P1 (the reviewer 18:21), #1360's merge, and the busy-room deafness (#828, the first builder's 19:55, with Fourth) | `second/waited-stop` | 2026-10-17 | #1352: the decided path held a waited sound that `on` had decided for want of echo evidence; such a block is kept only among the mask's last reads, so the loop's decision failed and the conversation ended; now it waits only on a block heard with echo evidence (`may_hold_once_decided`), the reviewer's probe a test failing at their error without it; a stop whose words are still out then goes as before, said in `group-talk.md`; the P3 a lamp-check step (`b612457d0`, Mac gate 1,918/0/11); #1360 rebased on Tests' #1350 (both rig fixes kept, the eighth's note taken), the control failing only the new test on ARM64, Tests' podman loop 10 of 10 on the held-up test, merged; peer-reviewed Third's #1358 (approve, three mutations failing its test); #939 was already done (the first builder's correction); for #828 Fourth's #1378 (`LAMP_SPOKEN_TO=on` in the product) already had the five-follow-ups test, so I peer-reviewed it: one P2 for the first builder's call, measured with and without the switch: two colleagues talking 45 to 180 s after the lamp's news are answered every line with it and held without it |
| 79 | The `write_timeout` drops (the first builder's 20:46: lamp-4ace's 105, 84 losing a line) and Step B, a lost line sent again, with Twelve (22:25) | `second/write-named`, `second/patient-writes`, `second/resend-lost-line` | 2026-10-17 | read from the code: every write to Gemini had 200 ms (a camera frame 400 ms, the look's 245 KiB picture the same 200 ms in one message), a write waits only when the link is behind, and a timed-out write ended the connection with nothing sent again; #1393 (merged): the drop names its write, size and bound (`write_timeout` with `write`, `kib`, `deadline_ms`); #1397 (off unless `LAMP_PROVIDER_WRITES=patient`): a longer bound alone only renames the drop (the 32-block queue, 640 ms of speech, overflows as `Backpressure`), so 1 s a write, 8 ms a KiB and a 128-block queue, a line over a 0.8 s stall kept; Nine's question (a confirmation that comes in during a write is not read before the barrier's quiet is judged, so `BarrierTimeout`) fixed by an owed read, behind the switch after the reviewer's P2, each part failing its test when taken out; Step B on Twelve's guards: lamp-gemini `Supervisor::try_resend_for_session` (a request lost before any output kept open once, opt-in), the worker keeps each line and holds a mendable loss after its end, then resends it on the next session as the same request with its second `Started` not handed on; only an ended line, as the coordinator gives an open one up on the link's change; Mac gates green; nothing on a lamp |
| second-P1 | (kept mergeable) the note after an early end, #738 | `second/early-end-labelled` | 2026-10-14 | rebased onto `e12a6cc83` (`f84889c91`) beside #782's `continues`, each field behind its own switch; `lamp-voice` 1,241 passed, 1 timed motor test failed under load and passed alone 3 of 3; merged by the first builder at 19:42 (ARM64 green, 4,499 passed); merged beside the second voice, it broke `lamp-voice`'s test build (three of the second voice's test literals without `continues_cut_off`): mended by #851 (`second/start-fields`, merged as `426ad0991`) |
| 45 | (for #811) the install's two blockers in `lamp-update`, on nine's #815 | `second/install-gates` | 2026-10-15 | `e13001625`, two commits on #815's `73a1fffa8` for nine to fast-forward: every part in `lampos.target` carries setup's marker condition itself (a target's condition holds back nothing it pulls in), and install writes a setting only where it is missing or still the default it wrote; on a throwaway Ubuntu VM (systemd 255) with no marker, #815's units started all 14 parts at boot and the fixed ones none; the reviewer's two later rounds (a failed enable never retried; a stop between a write and its record) closed: the debt and each setting recorded before the write (`d64bae34e`), `lamp-update` 68 passed; a sixth round (two failed runs in a row) fixed (`d5c033983`, `lamp-update` 69 passed); the first builder fast-forwarded #815 to it at 22:04; draft #863 gave the ARM64 run (4,682 passed); #815 merged by the reviewer at 22:2x and #863 closed; #811 open for the bootstrap |
| 43 | The person's phone over Bluetooth (issue #735, `second-P4`) | none | 2026-10-15 | cancelled by the owner the same night before any work was kept; released on #735 |

## Proposed tasks

| Id | Proposal | Issue | State |
|---|---|---|---|
| second-P1 | The early end taken back: resumed from the session's state before the piece, or a labelled continuation, probed once against the service before anything is built | #723 | yes, but (first builder on #723, 20:20): the study and the probe first, the probe's lamp step read-only of the voice's settings, recorded speech, at most 20 requests, `--dry-run`; taken 2026-10-14 |
| second-P5 | The hello on arrival asks the Policy (the second half of #752) | #856 | filed 2026-10-15 20:4x; yes from the first builder at 00:02; pull request #883, ready (`9e7b7cbf9`, ARM64 green, 4,765 passed); in shadow on lamp-4ace from 2026-10-16 00:38 (first builder); the reviewer's 00:43 finding (a hello chosen in shadow and the voice's own greeting for it counted twice) fixed in `938a5d53b` (`lamp-policy` 172 passed); #883 and #873 both change `lamp-policy`'s `main.rs` and `policy.rs`, put together on a local branch so the second to merge follows the first at once; ARM64 green on `938a5d53b` (4,770 passed); with #873 on lamp-4ace from `second/pictures-hello`; rebased at 03:5x for `lamp-voice`'s `mod selftest` (`a16945b9b`, msgs 93, policy 172, voice 1,383 passed); the reviewer's 01:33, 04:55 and 05:22 findings (a late first sound, one own turn speaking twice, a slow answer counted as unasked) fixed in `ea38e6874` and `589e904b1`: whether a turn was asked for is known at its start, and only a turn the lamp began itself counts, once (`lamp-policy` 175 passed); for the owner's minute at 11:00; rebased three times on 16 October as `lamp-voice` moved (#943's `--act attention`, #950's `lamp-skills`, #830's `lamp/online`): `34ce7a8ed`; ARM64 green on `38a10c694` (5,296 passed); rebased for #937 and #954 (`bb0e75569`, ARM64 green, 5,384 passed); merged at 11:25 |

## Found, not mine

- **The body's glance from rest gets neither its lead nor its "from where
  it rested" on a lamp** (2026-10-14, the body's, the fifth builder's;
  said on #714). Under `--rest-glance on` both need the body to have
  counted 50 steps at rest, and every request from Expression resets that
  count; `lamp-expression serve --enable` asks every 20 ms, zero offsets at
  rest included. How to see it: the body's own `expression_run_with` with
  zero requests before and after the glance, as on a lamp (a scratch test,
  not kept), lifts the goal 50 counts, against 80 to 90 in
  `a_resting_lamp_glances_with_the_switch_and_comes_back_to_rest`, which
  asks only during the glance; lamp-4ace lifted 49 at 18:40. lamp-52e6's
  head needs the lead to start from below.

## Handed back

Newest first.

### Task 45, for #811: the install's two blockers (2026-10-15, on pull request #815)

Ready for lamp-voice: through #815 (`second/install-gates`, `e13001625`)

**Why:** the reviewer found, on nine's #815, that nothing held the lamp's
parts back before setup: systemd checks a unit's condition only when that
unit's own job runs, after the jobs it pulls in are queued, so
`ConditionPathExists` on `lampos.target` gated none of the 14 parts (the
ears would open the room microphone on a lamp never set up). And a rerun
of install, which is how the updater upgrades itself, wrote the build's
default over a lamp's own pins. Nine had paused for the night; the crate
is the updater's.

**What it is:** each unit `PartOf=lampos.target` carries
`ConditionPathExists=/var/lib/lampos-setup/done` in `[Unit]`;
`lamp-update.service` and `lamp-records.timer` stay outside, ungated.
`state.json` gains `settings`, the SHA-256 of each product setting as
install last wrote it; install writes a setting only where it is missing
or still exactly that, and keeps and says any other.

**Tests** (M2 Max): every unit in the target gated, by `PartOf` and
`WantedBy` alike, 15 in all (fails with the condition taken out of
`lamp-ears.service`); the reviewer's case, line 99 to 100 and install
again, kept and said, nothing written or asked (fails with the old
always-write); a newer default replaces only an untouched setting; a state
from before reads and writes as before. `lamp-update` 63 passed, strict
clippy clean.

**What to run on the lamp:** a lamp with the product units and no marker:
reboot; `systemctl list-units 'lamp-*' --state=active` shows only
`lamp-update`, and `systemctl status lamp-ears` says "start condition
unmet". Then the marker and `systemctl start lampos.target`: every part.

**Against real systemd** (a throwaway Ubuntu 24.04 VM on the M2 Max,
Colima, systemd 255; the lamp's bookworm has 252): #815's units with no
marker started all 14 parts, by `systemctl start lampos.target` and by a
real boot, while the target itself was skipped; the fixed units started
none of them (only `lamp-update`, outside the target), and with the marker
all 14. The table is on #815.

**Where it is most likely wrong on the lamp:** systemd 252 behaving
otherwise than 255 here (not expected: condition checks have worked so
since long before both).

### Task 52: task news, polish (2026-10-15, issue #757)

Ready for lamp-voice: yes (#794)

**Kind: a fix, on by default** (the words of two notes and of one function
answer); a note that fits is byte for byte as before (pinned).

**What it is:** asked about news already told (the key in the told file),
`check_agent_task` answers `told_before: true` and asks for "it's the one I
mentioned", with a gist only if none was given; a question or a stop
already told is asked or said again shorter. A long result's note cuts the
task's words to 100 characters first, keeping 364 characters of a long
story against 190 before (a 273-character task, just finished; 324 against
150 a while ago), and asks for its gist in one plain line before "the rest
is on their computer", never only that.

**Tests** (M2 Max): the kept characters, the gist asked first and the task
cut to 100 for long and short tasks; told before, asked twice, a question
and a stop already told; a long or truncated result asked about. **Gate:**
fmt, strict clippy, `lamp-voice` 1,166 passed, 0 failed, 8 ignored (one
timed test failed once under load in the first gate and passed alone 3 of
3); ARM64 run 38703309129 green, 4,487 passed.

**What to run on the lamp:** the 08:02 part of the day again: a 300-word
story handed over by voice, told unasked, then "How is that task going?" a
few seconds later.

**Where it is most likely wrong on the lamp:** the model still giving only
where the rest is, or announcing the news again despite `told_before`.

### Task 48: the Policy decides when the lamp speaks first, progress news (2026-10-15, issue #752)

Ready for lamp-voice: yes (#765)

**Kind: off unless set:** `LAMP_PROGRESS_NEWS=on` on the voice and
`lamp-policy --progress --act progress`. Without the voice's switch no
subscriber appears on either topic, the tasks part is asked nothing new,
and no told file is written; without `--act` every progress decision and
log line stays `shadow:true` (tests). One commit, `e06d676d8`, on #751's
`8855a2507`; built by the sub-agent that built #751 and reviewed here
(only `task_news.rs` in the voice; subscriptions only with the switch, on
a thread of its own; each decision let go says why; the note through
`fitted`; the told key `<store>:<task>:progress-<change>` so it cannot meet
an observation's).

**What it is:** the policy, acting, sends progress decisions with
`shadow:false` while "is this said to me?" stays in shadow. The voice
takes a fresh `say_now` (60 s, this store, this task still working, no
newer change), fetches the progress line and the task, and offers one
note under its own gates, let go if not told within 60 s.

**Tests** (M2 Max): the chain from a change on `tasks/status` through the
real acting policy loop (a stand-in judge) to one note offered, told once
and kept; every kind of decision let go; long notes in English and
Vietnamese fit. **Gate:** fmt, strict clippy and tests on the 22 crates
that are or use `lamp-msgs`: 3,652 passed, 1 failed (`lamp-health`, the
Mac's disk warning), 21 ignored; the workspace build. Native ARM64 run
38667569523 on `e06d676d8`: green, 4,443 passed, 0 failed, 35 ignored.

**What to run on the lamp** (`docs/tasks.md`, "Playing progress news on
lamp-4ace"): both switches on after the minute; `act=progress` in the
policy's start line and a `voice.` subscriber on both topics; a task of a
few minutes handed over by voice while sitting in view; the change, the
`say_now`, the offer, the telling and the one sentence, with the time
from decision to sound.

**Where it is most likely wrong on the lamp:** Jev answering `wait` to
everything; the 60 s life and the voice's gates not meeting; a summary
line that does not make a natural sentence; the presence gate rarely
opening.

### Found in task 44's minute: news too long for one written turn (2026-10-15, for issue #736)

Ready for lamp-voice: yes (#756)

**What happened:** in #740's first minute on lamp-4ace the voice ended
three times in three, 4 ms after committing news, with `Gemini Live
InvalidConfiguration`. The news is one written turn, refused over 1,024
bytes (`wire::MAX_NOTE_BYTES`) as `InvalidConfiguration`, which the
provider worker ends on; the note's own words are 545 to 585 bytes and it
quoted up to 300 characters of the task and 1,200 of the answer. It
happened again at 07:26 without #740, and the first builder put the fix on
lamp-4ace at 07:38.

**Kind:** a fix, no switch: it changes only a note that would have been
refused; one that fits is the same byte for byte (a test). One commit,
`158968946`, on `lamp-voice` at `d128c458a`: `task_news::fitted` cuts the
note to fit, the answer first, then the task, by whole characters, marked
"…(it goes on)".

**Tests** (M2 Max): one byte over is refused by `encode_written_turn`;
every note, for long answers and tasks in English and Vietnamese, in every
state and at both ages, fits and is taken. **Gate:** fmt; strict clippy on
voice, voice-eval, memory; the `task_news` tests. Native ARM64 run
38657718552 on `158968946`: green, 4,410 passed, 0 failed, 35 ignored.

**What to run on the lamp:** the news of a task with a long answer, told
whole with no `InvalidConfiguration` (the first builder's 300-word story).

**Where it is most likely wrong on the lamp:** a long answer whose result
is at its end, cut away before the model sees it.

### Task 40: progress news through the tasks part and Jev (2026-10-15, issue #732, `second-P2`)

Ready for lamp-voice: yes (#751)

**Kind: off unless set:** `lamp-tasks serve --progress` and `lamp-policy
--progress [--utc-offset-minutes N]`. Without them no `ps` is run, the
`tasks/status` topic is never made, the policy subscribes to nothing new,
both diagnostics are exactly as before, `get_progress` is refused with
`progress_off`, and an attention decision's bytes are unchanged (tests).
One commit, `a3d10489b`, on `lamp-voice` at `1337e7e1d`; built by a
sub-agent of this session and reviewed here (no free text on the bus; the
`about` line kept only in the tasks part's memory; `ps` bounded in time,
bytes, rows and line length; the progress judge on its own thread and
connection). Rebased here once (a `docs/tasks.md` section beside the first
builder's).

**What it is:** `tasks/status` (`lamp-msgs` `TaskStatus`), one kept value
for every watched task: engine, state, seconds working, last active, how
often the progress line changed and how much, and signs read from it
without a model (`tests_pass`, `build_works`, `fixed`, `found`, `done`,
`pull_request`; a phrase table with words that cancel a phrase). One
`harness ps --machine <id> --json` per machine on the watch's schedule. In
the policy, "say this progress now?" asked of Jev in shadow (worth telling,
good moment, too much), with hard rules: never while anyone talks or within
15 s of speech, at most one note per task per 15 minutes, a yes that comes
back once someone talks turned to `wait`. Decisions on `policy/decisions`
with `about: "progress"` and `task: <store>:<task>:<change>`.

**Tests and measures** (M2 Max): 4 new in `lamp-msgs`, 11 in `lamp-tasks`,
10 in `lamp-policy`. `parse_ps` 55 µs for 40 harnesses; one read through
the adapter against a scripted `harness`, 4.2 ms median; status published
to decision on the bus with a stand-in judge, 52.7 ms median (folder
`docs/verification/progress-news-host-20261015/`). **Gate:** on the agent's
head, fmt, strict clippy on the 20 crates that are or use `lamp-msgs`, the
workspace build, and 3,159 tests passed with one Mac-only failure
(`lamp-health`, the Mac's disk 95% full); after the rebase, fmt, the same
clippy, `lamp-msgs` 79, `lamp-tasks` 115, `lamp-policy` 154 (2 ignored),
the build; merged onto `cf2f5b43c`, `cargo check --workspace
--all-targets`. Native ARM64 run 38632666318 on `a3d10489b`: green, 4,258
passed, 0 failed, 32 ignored.

**What to run on the lamp** (`docs/tasks.md`, `docs/policy.md`): time `harness
ps --machine office --json` ten times as `orangepi`; `--progress` on the
tasks part and `lamp-bus info tasks/status` before and after; one real
task with `tasks/status` tapped for two hours, its changes and signs
counted and each sign checked against `get_progress` by hand; the read
times from diagnostics; then a day of the policy with `--progress
--utc-offset-minutes 420`, its latency and choices, the disagreements
marked.

**Where it is most likely wrong on the lamp:** `about` being the person's
ask, not the agent's progress; `ps` (Node, up to 40 agents) costly every 2
minutes; English phrases not matching the agents' own summaries; Jev
uncalibrated for this question; "spoke up unasked" read from
`voice/state`; bus permissions between `orangepi` and the policy's dynamic
user, not checked.

### Task 5, the second model: the bare "Lamp" in voices it never learned from (2026-10-15, issue #694)

Ready for lamp-voice: yes (#750)

**Kind:** a model file, off unless installed: `models/name/name.onnx`
replaced (1,233,449 bytes, `a8453aeb…`), no crate changed. Two commits
on `lamp-voice` at `78b984b69`, ending `928803b08`; built by a sub-agent of
this session and reviewed here (sources and licences in `SOURCE.md`; no
recordings kept; `say` and Kokoro voices for testing only). Training code
under `lampOS/training/`, the exception flagged on #725.

**Why the first failed, and the change:** near misses were 54% of its
other-word negatives; now 10% of each batch's other words, and three seeds
averaged in one graph. `training/name/played.py` imitates the lamp's played
check in 33 `say` speakers it never trained on. At 0.9 (pass: 3 of 6
calls, at most 1 of 8 near misses) the first passes 9/33 as said and 8/33
played; this one 22/33 and 12/33, hearing 4.0 and 3.5 calls against 2.3
and 2.0. False alarms per hour lower on every set (LibriSpeech 1.4 against
3.3); near misses in Piper voices higher (Lan 40% against 25%). Scores
under tract through `hear.rs` match onnxruntime within 4e-6. **Gate:** fmt
and the workspace build (no crate changed). Native ARM64 run 38637337785
on `928803b08`: green, 4,277 passed, 0 failed, 33 ignored.

**What to run on the lamp:** `lamp-name check DIR RECORDING.wav` on the
23:47 recording, beside the installed model; install only if it hears at
least as many calls (3 of 6 offline) with no more false hits. Asked on
#694: the installed `name.onnx` (`1d238ca7…`), and the `say` voice, speed
and full lines played.

**Where it is most likely wrong on the lamp:** colleagues called Lan or
Lâm; played through a speaker only 12 of 33 speakers pass; no real person
and not the lamp's own voice tried.

### Task 45: updates, a lamp keeps its lampOS current by itself (2026-10-15, issue #737, MVP)

Ready for lamp-voice: yes (#741)

**Kind: off unless set:** a new part, `lamp-update` (`crates/update`), its
unit template (`deploy/units/lamp-update.service`, not in `lampos.target`),
and `--apply on|off`, default `off`. With it off it writes nothing, not
even its records, and never calls systemd or the bus (a test holds every
file under the lamp root byte for byte). One commit, `01ff1faeb`, on
`lamp-voice` at `c1b8c7591`; built by a sub-agent of this session and
reviewed here (the signature checked over the exact manifest bytes before
anything is read from it; names restricted to `lamp-<plain>` and
`lamp-<plain>.service`; files' URL signed, https or `file:///` only, no
`..`; downloads only to https, bounded by the listed size, hashed as they
come; no new crate in the lock).

**One round:** the signed channel envelope read (Ed25519 with `ring`, the
lamp's key from setup), only a newer build and never one put back once;
each changed part installed beside the old in `/opt/lampOS/<part>/<sha256>/`
after a disk check (a whole second copy plus 256 MB); the units switched
at a quiet moment (voice resting, 45 s with nothing heard or said), the
voice first; 60 s of systemd and `lamp/health` watched; every changed unit
put back in reverse if a part is unwell. V1's OTA weighed in the
`v1-verdicts.md` form. The publishing proposal (a prerelease per tested
`lamp-voice` push, one channel-head release with the newest signed
envelope, the secret `LAMPOS_UPDATE_KEY` named and not added, the CI job's
YAML in `docs/updates.md`) and three questions are on #737.

**Tests** (M2 Max): 29 unit (1 more ignored, the network) and 15 whole
rounds against stand-ins (switched; a carried unit; bad signature and
checksum refused; a dying part and one in error put back; no network; not
switched while someone talks; a full disk; an older build; a switch cut
short put back; its own folders removed, hand-installed kept). The
network test run once by hand against the real `models/ambernet` asset;
`manifest` on run 144's real artifact (31 binaries, 146.95 MB) and `check`
against lamp-4ace's units (nine changes planned, nothing written).
**Gate:** fmt; strict clippy on `lamp-update`; its tests; the workspace
build. Native ARM64 run 38616519206 on `01ff1faeb`: green, 4,251 passed,
0 failed, 32 ignored.

**What to run on the lamp** (`docs/updates.md`): `df -h` and the units
written down; a throwaway key and a local `file:///var/tmp/lampos-channel/`
from the next `lamp-voice` run; `lamp-update check`; the minute with
`--apply off`; one attended `once --apply on`; optionally a deliberate
rollback.

**Where it is most likely wrong on the lamp:** a minute of health missing a
bad but running voice; carried units from the repository's
`deploy/lamp-4ace/`, which disagrees with the lamp, undoing a setting made
there; the disk (14% free on 2026-10-14); quiet needing the voice's bus;
`daemon-reload` and `restart` from inside `ProtectSystem=strict`, never
tried.

### Task 44: a yes said in two breaths still starts the task (2026-10-15, issue #736, MVP)

Ready for lamp-voice: no (#740 closed as not needed, 2026-10-15)

**Closed.** In its first minute on lamp-4ace (02:28 to 02:37) nothing was
carried: the call came while "Yes." itself was still being heard, a shape
this does not carry, and the voice ended on long news (#756). The first
builder's `d128c458a` then answered such a call as not done because the
person was still speaking, and five of five two-breath yeses started the
task (07:22 to 07:32). That answer also covers the shape below, one model
round trip later, without letting "Yes... no, wait" through, so carrying
was not worth its path in the transport. The branch is kept at `0ebd994d5`
(on `e4922a3cb`, with the call records the first builder asked for on
#736, and the window test given a barrier bound past the window, after it
failed 3 times in 5 at the 2 s default).

What follows is the hand-back as it was.

**Kind: off unless set** (`LAMP_CARRY_CALLS=on` in the voice's unit, read
by its provider worker into `lamp_gemini::SessionConfig::carry_calls`). One
commit, `df5e5f348`, on `lamp-voice` at `c1b8c7591`.

**The cause**, from the code (the 22:37 record lines are asked for on the
issue): a function call belongs to the request whose input has ended
(`crates/gemini/src/session.rs`, `tool_calls`). "Yes." was committed;
"please start it." began a moment later and "Yes." was given up as
continued, its response becoming the barrier. The model's
`confirm_agent_task` for "Yes." came while "please start it." was still
open, which can own nothing, so it was answered `{"error":"cancelled"}` and
dropped (`UnownedToolCall`), and the model said it could not start.

**The change.** With the switch, such a call (while the barrier stands for
the request the open one replaced, that request continued and not
discarded, within 3 s) is the open request's `ToolCall`, marked
`carried_from`, and its answer is written when given; every other unowned
call is cancelled as before. The voice runs it as that request's, except a
call that changes what the person sees (the executive), which is not done
and the model told the person went on. A task's yes is still checked by
the voice's own rule.

**Tests** (M2 Max): transport: the 22:37 case carried and answered once;
refused without the switch, for an abandoned request, and after the
window. Voice worker: a carried `light_on` answered "not done" with the
executive never asked; a carried call for the time answered. **Gate:**
fmt; strict clippy on gemini, voice, voice-eval, policy, surround; tests
one at a time: gemini 259 (2 ignored), voice 1,043 (6 ignored), voice-eval
284, policy 144 (2 ignored), surround 260; the workspace build. Native
ARM64 run 38616463482 on `df5e5f348`: green, 4,213 passed, 0 failed, 31
ignored.

**What to run on the lamp:** with `LAMP_CARRY_CALLS=on`, the two-breath yes
five times and a one-breath yes five times, each after a prepared task;
the tasks started, what the lamp said, and "carried over" in the journal.

**Where it is most likely wrong on the lamp:** "Yes... no, wait" for a task
still starts it (the carried yes passes the voice's check); a task call
answered after the 2.5 s kept-start patience may be withdrawn by the
service and its answer dropped as stale.

### second-P1, step 2: the note after an early end (2026-10-15, issue #723)

Ready for lamp-voice: yes (#738)

**Kind: off unless set.** Only with `--early-end-ms`, which no unit sets
since 13:46: the successor of a request whose end was sent early and given
up starts with `continues_cut_off`, and the provider worker writes
`CUT_OFF_NOTE` beside it ("... this is the rest of it. Answer all of it as
one request."), with any other note for that request. The flag is left off
the wire when false: a start without it is byte for byte as before (a
test). One commit, `9417bcfcc`, on `lamp-voice` at `dcc2e6b1e`.

**The rest, not the whole.** Lamp-4ace answered the whole question with
its note as one, but the voice cannot send a piece's sound again: the
transport takes no sound older than 2.5 s (`MAX_INPUT_AGE`), and the pizza
question came in four pieces. So the successor carries the rest with the
note, and the probe now asks exactly that (`rest-labelled`, a new
`rest.wav`, in place of `resume-late`); a test holds the voice's note and
the probe's `REST_NOTE` to the same words. The probe also reads the voice's
own provider file as it is, as the first builder asked.

**Tested, on the M2 Max:** the early-end session tests see the note once,
just before the successor's end, when speech resumes 0.3 s after the end
and in "Lamp, stop." over an answer, and none on a request quiet to the
end; the probe's six tests (the voice's provider file read; a loose
credential refused). **Gate:** `cargo fmt --all --check`; strict clippy
on `lamp-gemini`, `lamp-voice`, `lamp-voice-eval`, `lamp-policy`,
`lamp-surround`; tests one at a time: gemini 256 (2 ignored), voice 1,037
(6 ignored), voice-eval 284, policy 144 (2 ignored), surround 260; the
workspace build. Some voice session tests fail on this Mac only while it
is loaded by other work; they passed alone and on quieter runs. Native
ARM64 run 38604441393 on `9417bcfcc`: green, 3,953 passed, 0 failed, 31
ignored.

**What to run on the lamp:** `rest.wav` ("Japan?", `say -v Daniel` and
`afconvert`), the probe once more with the voice's own provider file, and
`rest-labelled` must answer Tokyo without answering "the capital of" on
its own. Only then the five pause scenes with `--early-end-ms 200`
against a run without it, by the table in `docs/early-end-taken-back.md`.

**Where it is most likely wrong on the lamp:** the rest alone being weaker
than the whole; the note arriving after the given-up answer has begun
under `kept-start` (where the moon thought went wrong at 13:45); thoughts
in several pieces; short pieces heard as another language.

### second-P1, step 1: the early end taken back, study and probe (2026-10-14, issue #723)

Ready for lamp-voice: yes (#731)

**Kind: off unless set.** No library or service code changes: an example
that runs only when started by hand (`crates/gemini/examples/early_end_probe.rs`),
`service_probe`'s provider and WAV loaders moved unchanged into
`examples/shared/provider.rs` for both probes, `[[example]] test = true` in
the gemini manifest so `cargo test` runs the probe's tests, and
`docs/early-end-taken-back.md` with a pointer from `faster-answers.md`.
One commit, `0839cfb4b`, on `lamp-voice` at `3e3de2d7a`.

**The study** (`docs/early-end-taken-back.md`): the Live API has no
message that takes a request back (an `activityEnd` ends it, client
content "is unconditionally appended to the conversation history"). A
resumption handle "represents a state that can be resumed" and is valid
2 h after the session ends, but neither Google page says whether an
earlier handle resumes once later ones were given. So (a), resuming from
the handle before the piece, and (b), a note on the whole utterance, are
both open, and only the service can say.

**The probe:** five trials of three requests each (15, never over 20,
ended within 115 s): `control`, `resume`, `resume-late`, `whole-resumed`,
`whole-labelled`. It reads the voice's provider file by path (never
printed), sends five recorded utterances from files and opens no
microphone or speaker. It prints one JSON line a trial of timings and the
service's words, and `--dry-run` prints the plan without connecting. The
gemini library keeps only the newest handle and its handles cannot be
compared outside it, so the probe keeps its own copy of the handle from
before the piece, and resumes once at once and once after the piece's
whole answer, when a newer handle has surely been given.

**Tested, on the M2 Max, against the scripted service, not the live one.**
Five tests, the service given a memory that behaves as (a) needs:
`control` names orange and purple; `resume` and `resume-late` orange
only, each presenting the handle from before the piece; both whole-question
trials say Tokyo, and the note goes once inside the whole question;
nothing private printed; a refused resume reported and not retried; a
silent service ends the run within 115 s and 20 requests; the dry run
shows 15 requests and the note. The built example's `--dry-run` also ran
here on a placeholder provider file.

**Gate:** `cargo fmt --all --check` passes; `cargo clippy -p lamp-gemini
--all-targets -- -D warnings` clean; `cargo test -p lamp-gemini` 255
passed, 2 ignored (the probe's 5 among them); `cargo test -p lamp-gemini
--examples -- --test-threads=1`: the probe's 5 and `service_probe`'s 23
passed; `cargo build --workspace` passes. Native ARM64 run 38587667135 on
`0839cfb4b`: green, 3,822 passed, 0 failed, 29 ignored.

**What to run on the lamp** (`docs/early-end-taken-back.md`): the five WAV
files (`say` and `afconvert` on a Mac; in this session `say` gave 5 ms
files, so check that each is a second or two), `cargo build --locked
--release -p lamp-gemini --example early_end_probe`, then as the voice's
user `early_end_probe /etc/lampOS/voice/provider.json DIR --dry-run` and
the same without `--dry-run`. The document's table says which way holds
from the rows; step 2 is built only on that.

**Where it is most likely wrong on the lamp:** (1) the configured endpoint
not passing `sessionResumption` both ways, so a refused resume would be
the road and not the service; (2) no current handle within 3 s of the
first answer (`handle_before_piece: false`); (3) the model guessing the
words from the check question itself: the words in each row decide, not
the `answer_mentions` flags; (4) a slow link ending the last trials as
`skipped`.

**Not shown:** anything from the live service; a person's voice; how long
resuming takes on the lamp's link; what a resumed session keeps beyond the
probe's own instruction.

### Task 36: always alive, the idle layer, sleep and waking (2026-10-14, issue #714)

Ready for lamp-voice: yes (#729)

**Kind: off unless set.** Nothing changes unless Expression is started
with a catalogue that has an `alive` part
(`crates/expression/alive-behaviours.json`, lamp-4ace's catalogue with
one). Without it the engine has no idle layer at all (`life` is `None`)
and every existing Expression test passes unchanged; the new inputs it
subscribes to (`controls/gesture`, `schedule/due`) are read and ignored.
`serve` refuses a catalogue with sleep unless `--utc-offset-minutes` is
given.

**Built** (one commit, `928ad1a57` on `second/always-alive`, on `lamp-voice`
at `b9eadf847`), all in `crates/expression` (the third builder's crate; the
task names it) and its documents:

- `src/alive.rs`: the `alive` catalogue part with its bounds, and the
  layer: a breath phase-locked to the ring's own (the same function of the
  boot clock as `ring_face.rs`, so no message between them), drifts every
  30 to 90 s from a fixed hash (no randomness), in and out over 0.35 s,
  none while a face stays close and still under the light; sleep at night
  after 15 minutes of nobody, a slow breath and a bow, no clip begins;
  waking by the name, an enabled tap or stroke, a newly due reminder, its
  own answer, or the first face of the morning, with a 2.7 s stretch that
  crosses back to the normal plan; energy as size and pace.
- `engine.rs`: three inputs (`Touch`, `Due`, `DayTime`), the layer laid
  under the plan, clips held while asleep. `runtime.rs` subscribes to the
  two topics and tells the time of day every minute (`with_local_time`).
  `main.rs`: `serve --utc-offset-minutes`, `replay --long`,
  `import --long --utc-offset-minutes`. `replay.rs`: the long bounds (an
  hour and five minutes), the time of day added to an import, and a
  summary of each behaviour's ticks and starts, the largest change in one
  tick and the farthest asked.
- `docs/alive.md` (new), `docs/expression.md` (a section, the inputs,
  the long replay).

**Measured, on the M2 Max laptop, not on a lamp.** The tests: 16 through
the engine and 6 of replay, import and the live service (in
`docs/alive.md`, "Measured"). A written hour through the built
`lamp-expression replay --long`: by day, breathing all hour and 53 drifts,
the largest change in one tick 0.0022 rad (the layer coming in); at night
from 22:45, asleep at 23:00 after 15 minutes of nobody, 14 drifts before
it, none while it nodded off, and no change in one tick larger than the
layer's coming in.

**Read in the body, not changed:** at rest lamp-4ace's head may lift 50
counts but dip only 15 and roll 1, so the resting layer only lifts the
head; with the planner asking every 20 ms the body follows small slow
lifts a count a step, and a zero stream writes nothing (a scratch test on
the body's own harness, not kept).

**Gate**, on the M2 Max: `cargo fmt --all --check` passes; `cargo clippy
-p lamp-expression -p lamp-lean -p lamp-voice --all-targets -- -D
warnings` clean; `cargo test -p lamp-expression` 203 passed (23 new);
`cargo test -p lamp-voice -- --test-threads=1` 1,027 passed, 6 ignored;
`cargo test -p lamp-lean` 61 passed, 2 ignored; `cargo build --workspace`
passes. The voice and lean tests ran before the last two changes inside
`alive.rs` (the slow fade into sleep, no drift while the layer goes out),
which neither uses. Native ARM64 run 38581248942 on `928ad1a57`: green,
3,838 tests passed, 0 failed, 29 ignored (its 255 `test result` lines).

**What to run on the lamp**, smallest first (`docs/alive.md`, "What to
run on the lamp"): (1) record a quiet hour with `lamp-bus record ...
--seconds 3600 --max-mb 128`, `lamp-expression import --long
--utc-offset-minutes 420`, `lamp-expression replay --long --enable
--catalogue alive-behaviours.json`, and read the summary: no input
refused, breathing nearly every tick, about a drift a minute, the largest
step under 0.003 rad; (2) install the catalogue and add `--catalogue ...
--utc-offset-minutes 420` to the `serve --enable` command, the body
keeping `LAMP_EXPRESSION_BODY=on` and `--rest-glance on`, and its own
`--idle on` off for one idle author; (3) ten minutes at rest standing and
folded, and once at night (or with `night` and `nobody_us` set short).

**Where it is most likely wrong on the lamp:** (1) the breath too small
for the head servo to show (20 counts asked at 12 counts a second; a
25-count ask reached 12 to 14 on lamp-52e6), or shown in steps that read
as a servo hunting: `breath.inhale` to -0.045, or to 0; (2) the servo
heard at rest by the microphones the name listener uses; (3) sleep broken
by talk: Expression holds its own plan, but the body's listening pose
answers any turn the voice takes, so a sleep that holds needs the voice or
the body to know it (#713); (4) the 15-count bow unseen; (5) the work
light's face size and 5 s guessed.

**Found, not mine** (said on #714): with `--rest-glance on` and Expression
serving, the body never counts 50 steps at rest, so the rest glance's
"from where it rested" and 40-count lift lead never engage; the glance
lifts 50 counts against 80 to 90 in the body's own test (simulated
servos), as lamp-4ace's 49 at 18:40. It matters for lamp-52e6's head.

**Not shown:** anything on a lamp; a real quiet hour (asked on #714; the
hours above are written); energy and "asleep" from the inner state
(#713); the arm's stretch (the body's `stretching` clip); the ring's look
while asleep.

### Task 5: the name, heard as a bare "Lamp" (2026-10-14, issue #694)

Ready for lamp-voice: yes (#725)

**Kind: off unless set.** No code changes. The new `models/name/name.onnx`
is a file in the repository that nothing installs: `lamp-name` reads
`name.onnx` from its `--models` directory (`/opt/lampOS/name/models/` on
lamp-4ace), and nothing under `deploy/` or `crates/` names
`models/name/`. Until someone copies it there, every lamp hears its name
exactly as before.

**Built** (one commit, `78648897c` on `second/bare-name`, on `lamp-voice`
at `a9a7262a9`): `models/name/name.onnx` (124,135 bytes, 30,913 weights,
SHA-256 `f376e964…02030`, with `SOURCE.md`), the shape of the name model
`lamp-name` already loads ([1, 16, 96] openWakeWord embeddings in, one
score out), so it replaces the lamp's file with no code change; the
document `docs/bare-name.md`, and one paragraph in `docs/name.md` pointing
to it; the training code `training/name/` (Python, `sh make.sh` end to
end, each download checked against `downloads.sha256`). It reuses task 3's
`training/stop/` by import, with three additions there that are off by
default, so task 3's results do not change: `store.py` and `train.py` take
another folder (`--inputs`), `train.py` can leave out one way of calling
(`--leave-out-way`), and `make.sh` stops after the shared sets when
`STOP_ONLY_CLIPS` is set. Trained only on speech whose licence allows
shipping the model (Piper's LibriTTS voices, MSWC English, Speech Commands,
LibriSpeech, FLEURS Vietnamese: CC BY 4.0 or MIT). EdAcc (CC BY-SA 4.0) and
AMI were used for testing only. A third of every generated line is read
with espeak's Vietnamese phonemes as a stand-in for a Vietnamese accent; no
licence-clean recording of Vietnamese speakers saying English could be
trained on (the table in the document says why each was not).

The work was done by a sub-agent of this session on the M2 Max. I
reviewed it and moved its four commits onto the newest `lamp-voice` as
one; the training code's first version read FLEURS's 32-bit floats as
silence, and that was mended and the model retrained before it was handed
over (the document says so under "What went wrong on the way").

**Measured, on the M2 Max laptop, not on a lamp** (`docs/bare-name.md`,
"Measured": voices and speakers never trained on, a score of 0.9 for one
step, the rule `lamp-name` uses now):

| Way of calling | English reading | Vietnamese-accent stand-in | Real people (MSWC "lamp", 51) | Both microphone effects (English / accent / real) |
|---|---:|---:|---:|---:|
| "Hey Lamp, ..." | 94 % | 87 % | | 95 % / 84 % |
| "Lamp ơi, ..." | 92 % | 76 % | | 89 % / 77 % |
| "Lamp, ..." | 86 % | 53 % | 51 % | 72 % / 42 % / 63 % |

A voice from another generator (Piper's British Cori, never trained on)
is heard far less: "Hey Lamp" 67 %, "Lamp ơi" 5 %, "Lamp," 16 %. The
comparison model, the same recipe without bare calls, does no better on
Cori.

What sets it off, at 0.9: "Lamb" at the start of a line in 30 % of
generated clips, "Lan" and "Lâm" 25 %, "lab" 11 to 21 %, "land" 9 to 17 %,
rhymes 3 to 14 %; "Sam", "Liam", "ma'am", "plan", "hey" and "lamp" inside a
sentence 0 to 2 %. Talk: 3.3 an hour of LibriSpeech readers, 10 of AMI
meetings, 23 of Vietnamese (FLEURS), 36 of Vietnamese speakers of English
(EdAcc). The lamp-like replies: 8.4 an hour, nearly all "Lamp here, what
can I do for you?" (24 of its 64).

Beside the same recipe trained without bare calls (the stand-in for the
lamp's own file, which is in no branch and was not on this machine): the
bare "Lamp," goes from 1 % to 65 % (all generated voices), "Hey Lamp" 90 %
to 88 %, any near miss 4.5 % to 9.4 %, readers 3.7 to 3.3 an hour, FLEURS
17 to 23, EdAcc 48 to 36, the lamp-like replies 1.4 to 8.4. At 0.98
instead of 0.9: the bare name 59 % / 31 % / 18 % (English / accent /
real), "Lamb" 5 %, readers 1.3 an hour, EdAcc 16.

**My own check of the file**, on the M2 Max: `lamp-name check` built from
`second/bare-name` (`--release`, `hear.rs` under tract 0.23.8) with
openWakeWord's two front-end files (SHA-256 as in `SOURCE.md`) and this
`name.onnx`, over the sub-agent's three test files of held-out clips laid
end to end: 18 hearings in 24 generated calls, 6 in 12 real "lamp" words,
2 in 24 near misses, at 1.4 to 1.5 ms for each 80 ms step of all three
models. That agrees with the tables; it is a laptop's time, not the
lamp's.

**Gate:** no Rust changed. `cargo fmt --all --check` passes; the native
ARM64 run on the branch's head (`78648897c`), run 38569922098: green,
3,785 tests passed, 0 failed, 29 ignored (summed over its 252 `test result`
lines). `lamp-voice` then moved nine commits that touch none of these files,
so the branch was not rebased and the run stays on its head.

**One rule read narrowly.** `builders/second.md` says the second builder
may not commit Python. The task's hand-back asks for `training/name/` "as
task 3", whose Python training code is on `lamp-voice`, and the model
cannot be made again without it. So it is in the pull request. If it is
not wanted, I take out `training/name/` and the three `training/stop/`
additions and keep the model, its source and the documents.

**What to run on the lamp**, smallest first:

1. Before installing anything, from a directory holding
   `melspectrogram.onnx`, `embedding_model.onnx` and this `name.onnx`:
   `lamp-name check THAT_DIR FILE.wav ...` over the lamp's own microphone
   recordings of calls (the 2026-10-14 ones in attention.md, or new ones:
   `lamp-bus record DIR mic2/audio --seconds 600`, then
   `training/stop/from_bus.py`), and the same files with the lamp's
   current models directory. It prints each hearing at 0.9 and the highest
   score per file: the eight bare "Lamp," lines that the current file
   heard in none are the ones to count.
2. Ten minutes of the lamp talking with nobody near, recorded the same way
   and run through `lamp-name check`: how often it hears itself.
3. Only then, as `docs/bare-name.md` "Wiring it on the lamp" says: keep
   the current file as `name-hey.onnx`, copy this one in its place,
   restart `lamp-name.service`; going back is the reverse.

**Where it is most likely to be wrong on the lamp:**

1. **A bare "Lamp" after the microphone's lost onset.** The first tenth
   of a second of "Lamp" is most of its "l", and the loss trained on is a
   guess (20 to 40 dB down for 60 to 140 ms), not measured from the lamp.
   The rows "onset lost" and "both" in the document are where to look
   first.
2. **The lamp's own voice.** "Lamp here, ..." set it off 24 times in 64.
   Gemini's voice through the lamp's loudspeaker and canceller was never
   heard. Whatever takes `name/heard` must pass over hearings while the
   lamp's own reply plays, if it does not already.
3. **Vietnamese voices, and Lan and Lâm.** Every accented call here is a
   stand-in, and Vietnamese talk was the hardest part before the bare
   name too (23 and 36 an hour). A colleague called Lan or Lâm sets it off
   about one time in four.
4. **Any real voice.** The model leans on the one generator it learned
   from (Cori: 16 % for a bare "Lamp"); real people's single word "lamp" is
   at 51 %. Real calls recorded at the lamp are what it needs next.

**Not shown:** anything on a lamp; the lamp's current `name.onnx` beside
this one (the commands to put it in the same tables are in the document,
"The lamp's current name.onnx"); a real person saying "Lamp," as a call;
more than one training run.

### Task 3: a listener for "stop" under the lamp's own voice (2026-10-14)

**Built:** `models/stop/stop.onnx` (124,135 bytes, 30,913 weights, SHA-256
`5fe07569…72d41`, with `SOURCE.md`), the same shape as the name's model:
the newest sixteen openWakeWord embeddings in, `[1, 16, 96]`, one score out,
every 80 ms. The training code is `training/stop/` (Python, `sh make.sh`
end to end, every download checked against `downloads.sha256`), and the
document `docs/stop-word.md`. Trained only on speech whose licence allows
shipping the model: Speech Commands, MLCommons' Multilingual Spoken Words
(English, from Common Voice), LibriSpeech and AMI (all CC BY 4.0), and
speech generated with Piper's `en_US-libritts-high` voice (trained from
scratch on LibriTTS; the LibriTTS-R voice openWakeWord uses was avoided,
its data being for research only). No Rust changed.

**The task is met for the word alone and not under the lamp's voice.**
Threshold 0.9, one step, speakers never trained on; "suppressed" means
through a suppressor told the other voice exactly, which the lamp's
canceller is not, so the lamp lies between the two:

| | alone | 0 dB | −5 | −10 | −15 | 0 dB, suppressed | −15 dB, suppressed |
|---|---|---|---|---|---|---|---|
| "stop", real speakers (736) | 85% | 17% | 11% | 7% | 4% | 48% | 25% |
| "wait", real speakers (263) | 84% | 13% | 8% | 4% | 3% | 48% | 18% |
| "hold on", generated (375) | 95% | 47% | 30% | 21% | 14% | 89% | 59% |

Plainly mixed under another voice the word is lost: a speech recogniser
(wav2vec 2.0) also wrote only the other voice's words in those mixtures,
so no threshold mends it. False alarms an hour at 0.9: 9.0 over 11.1 h of
read speech, 3.2 over 0.9 h of generated voices, 7.4 over 24.1 h of AMI
meetings; at 0.98 about 2.2, 1.1 and 1.0, but only 48% of words alone are
then caught. Near misses at 0.9: "don't stop" 3%, "bus stop" 3%, "stop"
inside a sentence 8%, "stopped" and "waited" cut from sentences 56% and
60%. "Stop it." counts as a stop (19 of 19 alone). In AMI meetings, said
naturally: 1 of 28 "stop", 3 of 39 "wait", 5 of 22 "hold on". Median 62 ms
from the end of the word to the hearing. On the M2 Max laptop, one
training run and one seed.

**Found on the way:** openWakeWord's mel model sets its floor 80 dB under
the loudest value of everything it is given at once, so features made in
batches differ from `hear.rs`, which gives it one chunk at a time;
training reproduces the chunk-at-a-time floor, and its embeddings match
`hear.rs` to within 4.6e-5. **Under tract:** a scratch program running
`hear.rs` unchanged under tract-onnx 0.23.8, with `stop.onnx` in place of
`name.onnx`, matched training's scores to within 6e-6 over 1,185 steps,
on macOS and in the Linux ARM64 container; the model alone takes 24 µs a
run there.

**Recommended in the document:** 0.9, one step, listened for only while
the lamp speaks; a hearing pauses the speaker and lets the duplex rules
confirm or resume. If a hearing is to stop the lamp outright, 0.98.

**Gate:** no Rust changed; `cargo fmt --all --check` clean on
`lamp-second` with these commits; `cargo build --workspace` passed on the
task's branch, and the code it builds is unchanged by them.

**On the lamp, the quickest look:** ten minutes of `lamp-bus record …
mic2/audio` while the lamp talks and nobody speaks, then `from_bus.py` and
`listen.py` from `training/stop/`: how often it fires on the lamp's own
voice through the canceller. With no code change, `lamp-name check` runs
the model on the lamp if `stop.onnx` is installed as `name.onnx` in a
directory of its own. Wiring (a second model on the same embeddings in
`lamp-name`, a topic of its own) is proposed in the document, "Wiring it
on the lamp".

**Where it is most likely to be wrong on the lamp:** the canceller. The
whole difference between 17% and 48% at equal level is how much of the
lamp's own voice is gone before the listener hears it, and the lamp's
second microphone and its chip are in no mixture made here. And it may
fire on the lamp's own voice where the canceller leaves it, which is the
first thing the ten-minute recording shows.

**Not shown:** the lamp, its microphone, its canceller, a person at the
lamp; real "hold on" (2 read clips and 22 in meetings); more than one
seed. Common Voice itself was not used (it now needs an account).

### Task 19: the end sent early, the answer played only when sure (2026-10-14)

**Built** (`crates/voice`; `docs/faster-answers.md` way 2,
`docs/conversation-latency.md`, `docs/live-runtime.md`,
`docs/conversation-thread.md`): `--early-end-ms MS`, off by default;
100 to 1,000 ms in steps of 10, shorter than the end-of-speech wait in
force, and refused without `--provider-continuation kept-start`. For the
lamp: `--early-end-ms 200`.

- At that much quiet the coordinator sends the request's end
  (`activityEnd`) and keeps listening. The authority goes on saying
  "listening" until the 600 ms wait, or the end-of-turn model's verdict,
  confirms; nothing can play before. The answer's sound is held and starts
  at the confirming moment, or at once if that is already past.
- Speech again before confirming (six blocks in a row at 0.8 or more, the
  detector's own start rule) gives the request up exactly as
  `commit_admitted_input` does: `user_interrupted`, then an Interruption
  admit with nothing published between, which kept-start treats as a
  continuation. The blocks heard since the end (at most 30) open the next
  request.
- An utterance that would count as a sound gets no early end, so a lone
  "Lamp," stays one request with its question.
- The model's words reach `voice/said`, and the body its gestures, only once
  confirmed. A "..." answer is ended unheard at the confirming moment; the
  busy-room floor waits for it.
- **Outside the coordinator, for the first builder to look at:**
  `provider_worker.rs` and `thread.rs` take a request given up after its
  end but before confirming out of the conversation thread (`thread::unsay`),
  so its words are not kept as said. It cannot happen with the option off.

**Tests against the scripted service** (real worker and session,
kept-start; "start/end" are activityStart/activityEnd):

| Case | Sent to the service | Given up | Last sound to first block to the speaker |
|---|---|---|---|
| Quiet to the end | 1 start, 145 blocks, 1 end | 0 | 604 ms, at the confirming moment and not before |
| Speech again at 0.3 s (vad-only, and duplex 600) | 2 starts, 235 blocks, 2 ends | 1 | about 1,271 ms, the second answer only; the first never played |
| Answer before confirming | 1 start, 1 end | 0 | 603 ms |
| A "..." answer before confirming | 2 starts, 1 end | 0 | nothing played |
| Answer after confirming (option on / off) | 145 / 185 blocks | 0 | 1,269 / 1,670 ms |
| The link drops between | the first link one request, the next question on a new link | 0 | the first turn `provider_failed`, nothing played; the next 603 ms |
| "Lamp, stop." over an answer | 3 starts, 3 ends | 1 | 602 ms |
| A lone "Lamp," a pause, the question | 1 request | 0 | 1,273 ms |
| Option off | end at 600 ms, no new records, the same worker arguments and `run_start` | 0 | 920 ms |

A transport test shows a name note goes only with the request ended early
and never reaches the next, continued or abandoned. Measured path from the
last sound to the first speaker block: with the injected clock at the
lamp's 1.15 s service time, 1,910 ms off and 1,510 ms on; in a scripted
session with a 900 ms service, the end sent at 604 against 203 ms and the
first block at 1,670 against 1,269 ms.

**Gate.** On the Linux ARM64 VM, from `lampOS/`, on `cd815e905`:
`cargo fmt --all --check` clean; `cargo test --locked -p lamp-voice-eval`
284 passed; `cargo test --locked -p lamp-voice` 955 passed (5 ignored), the
library and every test target but `capture_hold_worker`, which does not
compile on `lamp-voice` itself ("Questions", 1); `cargo build --locked
--workspace` builds. Clippy for `lamp-voice` and `lamp-voice-eval` lists
only the older `lamp-body` warning and that same test; with `-D warnings`
it is clean on the builder's Mac, where that test is not built. On the builder's Mac, the voice library passes test
by test, and in full parallel runs fails only on timing under load (motor
and ring timeouts, ENOBUFS; once one "Lamp, stop." session with
`ConnectionReset`, cause not found, diagnostics added): `origin/lamp-second`
fails the same motor and ring tests there in the same way.

**On the lamp:**

1. Build, deploy, add `--early-end-ms 200` to `ExecStart`.
2. `pauses.sh`: an `early_end` record on each line; "Hey Lamp," with a
   450 ms pause gives one `early_end_given_up` and is answered; a bare
   "Lamp," gives none; 650 and 900 ms pauses behave as before; the first
   sound about 0.4 s sooner.
3. `shorts.sh`: the same words stop the lamp as without the option, and
   the `not_speech` counts are unchanged.
4. `checks.sh`: all pass; give-ups only at pauses of 0.2 to 0.6 s.
5. Watch the journal for `BarrierTimeout`.

**Where it is most likely to be wrong on the lamp:** how the real service
treats a request committed and given up 0.3 s in, and what the model makes
of the rest. And a first piece the service never answers ends the link
(`BarrierTimeout`) unless `provider.json` sets `silent_response:
assume_over` with `no_reply_after_ms`; whether the lamp's file does is not
known here.

**`crates/gemini`:** nothing needed. One possible speed-up: if the real
service confirms an interruption sent before any output, `retire_active`'s
kept start could be released at once, and short continuations would be
answered sooner.

**Not shown:** anything on the lamp or with the real service; an answer
that arrives and completes whole before confirming is still written to the
memory journal (with the lamp's 600 ms wait and no end-of-turn model that
cannot happen).

### Task 20: the replay takes the lamp's own recordings (2026-10-14)

**Built** (`crates/audio/src/replay.rs`; `docs/steady-reference.md`, "The
replay takes the lamp's recordings"):

- `lamp-audio replay-aec` follows a capture-clocked recording through. It
  rebuilds the voice's ring as `ReferenceAssembly` does: the lead and the
  prime at each start or reset; before each capture block, every frame
  accepted by then (`reference.accepted_through`); one block taken; the
  rate for the next from the fill and the frames owed since the last packet
  (`analysed_at_us` less `accepted_at_us`, at most 20 ms). After every
  block its cursor must equal the lamp's (`reference.analysed_through`), so
  the canceller gets the lamp's blocks in the lamp's order.
- It knows the mode from `run_start` in `events.jsonl` (`reference_clock`),
  or, for a session too old to say, from the first capture record's
  cursor; `report.json` says which and how.
- Where it cannot follow, it stops at that capture record, names it and
  why in `limitations`, and keeps and writes out everything before it.
- `events.jsonl` is read a line at a time (up to 4 GiB, which a long serve
  session may need; it was read whole under 32 MiB before): a cut-off last
  line is passed over and reported (`events_cut_off`); a line over 64 KiB
  is counted (`events_long_lines`); any other bad line is an error naming
  its number.
- Without an option the cancellers get the reference the recorded voice
  used (`echo_reference` in `run_start`); `--steady-reference` or the new
  `--as-given-reference` chooses.

**Tested** in `crates/audio/tests/replay.rs` on recordings made in the
tests as the voice makes them: a capture-clocked one of 300 blocks (the
speaker 1,000 ppm fast, every seventh packet late, so the rate moves)
replays whole, `aec_only_mismatched_samples_vs_recorded` 0; the same
without `run_start`, known by its cursor, exact; a reset in the middle,
two segments, both exact; events cut off mid-line, read and reported; a
cursor moved by one frame at the 61st record, stopped there, the record
named, the 60 blocks before kept and exact. The 12 older replay tests pass
unchanged. Gate on the Linux ARM64 VM, on `cb4d24e61`: `cargo fmt --all --check` clean;
`cargo clippy --locked -p lamp-audio --all-targets -- -D warnings` clean;
`cargo test --locked`: `lamp-audio` 178 passed (2 ignored), `lamp-voice-eval`
284 passed, `lamp-voice` 935 passed in its library and every test target
but one; `cargo build --locked --workspace` builds. The one is
`crates/voice/tests/capture_hold_worker.rs`, which does not compile on
`lamp-voice` itself (`1766259f3`): line 161 calls `capture_on` with nine
arguments, and it takes ten since the steady reference was wired
(`--echo-reference`). This change touches nothing in `crates/voice`; see
"Questions".

**On the lamp** (any recording made with the audio diagnostics):

```sh
lamp-audio replay-aec RUN artifacts/replay-as-recorded
lamp-audio replay-aec RUN artifacts/replay-steady --steady-reference
```

The first should end `replay_complete_for_recorded_capture: true` with
`aec_only_mismatched_samples_vs_recorded: 0` in every segment (the lamp
runs without noise suppression). If so, compare the second's
`lamp_sounding_internal_alignment_ms` and `…_changes` with the first's,
and listen to `segment-*-aec_only.wav` at the words played over the lamp.

**Where it is most likely to be wrong on the lamp:** a kind of record the
lamp's recordings have and the made ones do not. The made recordings
follow the recorder's format as `replay.rs` checks it, not the voice's
recorder run against scripted devices (no test in `crates/voice` does that
in a capture-clocked session, and that crate is not this task's).
Candidates: recovery (`--reference-recovery`, off on the lamp but its
reseeds would move the ring), a privacy close and reopen mid-run, or the
fourth recording's events failing for a reason other than a cut-off or
long line (the message now names the line). The first mismatch's
`limitations` line names the record.

**Asked for:** two of the lamp's capture-clocked recordings with played
voices only (for example `cut-a` and the run of words into quiet), as a
`.tar` attached to a draft release of this repository, or wherever is
easiest; say where in "Answers". With them I will make the made
recordings' shape match whatever the lamp's have.

### Task 10: a reference that follows the microphone's clock (2026-10-14)

**Built.** `lamp_audio::steady::SteadyReference`, off unless asked for:
`EchoProcessor::with_reference(noise_suppression, Reference::Steady)`
(`EchoProcessor::new` is unchanged), and `lamp-audio replay-aec RUN OUT
--steady-reference`. A second echo-evidence observer follows where the
echo is from the sound itself; the reference is delayed, through `drift`'s
interpolator and at most 2 ms a block, so that the canceller finds the echo
20 ms after it and keeps finding it there. Everything is in
`docs/steady-reference.md`.

**Why it follows the sound and not a buffer.** Read from the code: in the
capture-clocked mode the lamp runs, a jump in the speaker's queue goes into
the ring too and cannot step the echo; the recorded steps of 5.4 ms back
and forth are most likely the second microphone's own buffering, which the
lamp cannot read; and the delay hint the voice passes leaves the speaker's
queue out in that mode (the queue less the ring's frames is always below
zero and floored), so the hint is the capture delay alone.

**The rig** (`crates/audio/tests/steady_reference.rs`): speech-like sound,
the speaker 98 ppm fast, the microphone 200 ppm slow, the delay stepping as
recorded, a person over the lamp, and the voice's capture-clocked ring as
`ReferenceAssembly` drives it. **The two measures**, before → after, with a
moving echo (M2 Max and the Linux ARM64 VM, identical figures):

| Person against the echo | Echo removed | Person kept over the lamp | Echo within 1 ms of its place |
|---|---|---|---|
| level | 46.5 → 44.6 dB | −5.9 → −3.7 dB | 7% → 76% |
| 15 dB under | 45.3 → 43.3 dB | −8.5 → −7.1 dB | 7% → 81% |
| level, half the echo not linear | 43.9 → 40.4 dB | −5.6 → −3.7 dB | 7% → 71% |

A still echo costs a person 3.8 dB; the moving one 5.9; steadied, 3.7. The
canceller's own estimate goes from 28–84 ms to 0–20 ms. The rig shows the
direction and size, not the lamp's figures: on the rig the canceller copes
with a moving echo better than on the lamp. Cost of the stage: 6 µs a 10 ms
block on the M2 Max, 12 µs on the VM (99th percentile 24 and 52 µs).

**Tested** on the Linux ARM64 VM, from `lampOS/`, on `5d8093a8e` rebased on
`lamp-second`: `cargo fmt --all --check` clean;
`cargo clippy --locked -p lamp-audio --all-targets -- -D warnings` clean;
for `lamp-voice` and `lamp-voice-eval` the only clippy warning is the same
one in `lamp-body` as before (`run` in `crates/body/src/inspection.rs`
never used), in a crate this change does not touch; `cargo test --locked`:
`lamp-audio` 173 passed (2 ignored: the release-only measures and an older
one), `lamp-voice` 937 passed, `lamp-voice-eval` every suite passed, no
failures; `cargo build --locked --workspace` builds. The measures above
come from `cargo test --release -p lamp-audio --test steady_reference --
--include-ignored --nocapture` on both machines.

**On the lamp:** first the replay, twice, over the recordings of
2026-10-14:

```sh
lamp-audio replay-aec RUN artifacts/replay-given
lamp-audio replay-aec RUN artifacts/replay-steady --steady-reference
```

Compare, per segment of `report.json`, `lamp_sounding_internal_alignment_ms`
and `..._changes` (the canceller's estimate: it should hold within a few
milliseconds and change rarely with the option, against 80 to 128 ms as
given), and listen to `segment-*-aec_only.wav` at the words played over the
lamp. `operations.jsonl` has `steady_reference.echo_ms` for every block: it
should step by about 5.4 ms where the recording's echo stepped. If it does,
wire it: where the capture worker makes `EchoProcessor::new(…)`, make it
`EchoProcessor::with_reference(…, Reference::Steady)` behind a voice option.

**Where it is most likely to be wrong on the lamp:** the follower. It
needs a linear fit at one alignment to explain at least half of the
microphone's energy, and on the second microphone little of the echo is a
linear copy; where it explains less, the reference keeps its last delay.
And a match one pitch period of Gemini's voice away may be held for a while
(about a second per step on the rig). `echo_ms` in the replay shows both.

**Not shown:** the lamp, its second microphone's chip, a person, whether
2 dB is heard, the cost on the lamp's processor, the stage inside a running
voice. The causes of the movement are read from the code, not measured.

### Task 9: faster answers, what exists (2026-10-14)

A study, `docs/faster-answers.md`. Nothing built, nothing run on a lamp,
no paid service called. Every outside figure is marked "published, not
measured here" with its source and boundary.

**What it found.** The lamp's speech service (`gemini-3.8-live`) is
published at 1.18 s from a question to its first sound; the lamp sees
1.15 s from the end it sends. So the relay and the road from Ho Chi Minh
City are probably a small part of the wait, and no setting brings this
service near 0.3 to 0.5 s. What the lamp can take off is its own: the
0.6 s it waits to be sure the person has finished, and part of the 0.35 s
it holds the first sound. That gives about 1.5 s. Near 1 s also needs a
faster service, which today means another voice, against the owner's
decision.

**The ways, in short:** (1) end a plainly finished turn sooner: task 1,
take first; (2) send the end at about 0.2 s of quiet and play only once
sure: up to 0.4 s on every answer, code in the voice, behind a switch;
(3) a shorter hold before the first sound: about 0.1 s at 200 ms; (4) the
same service without the relay: unknown, probably small, needs a key;
(5) another Gemini Live model or an Asian endpoint: up to 0.5 s by
published figures, but older, preview or retiring models; (6) another
speech-in, speech-out service: a new voice, no; (7) speech to text, a
text model and a voice: not shown faster from Vietnam; (8) shipped
agents' own figures: a yardstick only (1.0 to 1.7 s median measured from
the user's side).

**For the first builder, in the study's order of trials:** first split
the 1,034 recorded answers into wait, service and hold, and time the road
from the lamp (it changes nothing on the lamp); then the fast end-of-turn
model; then a 200 ms hold; then the end sent sooner.

**Not established:** no round trip from the lamp is recorded anywhere;
where the relay's own server is; where Google serves an AI Studio key;
whether Gemini's text-to-speech "Kore" sounds like the Live voice; the
company's bill through the relay.

### Task 1: the end-of-turn model in a tenth of a second (2026-10-14)

**Built.** `lamp-turn-end --fast`: the same pinned model
(`smart-turn-v3.2-gpu.onnx`, unchanged), made 8-bit in memory as the worker
starts and run by ONNX Runtime 1.30.0 on two threads, the worker kept on
the cores with the largest `cpu_capacity` (cpu6 and cpu7 on the A733).
Without `--fast` the worker is what it was, byte for byte in its answers.
Also `lamp-turn-end bench` (the timing the task asked for),
`lamp-turn-end agree` (fast against exact on labelled clips),
`lamp-turn-end --check-model PATH --fast`, and
`WorkerCommand::smart_turn_fast` in `crates/turn-end-wire` for the voice
to start it with. Nothing in the voice uses it. Everything is in
`docs/endpoint-pilot.md`, "The fast model": what was tried and why each
was or was not taken, the measurements, the agreement tables, the cost of
the native library, and what was not shown.

**What it showed, on the Linux ARM64 VM** (M2 Max host; not the lamp):

| | Today (exact) | Fast |
|---|---|---|
| Verdict, features and model, median of 24 warm runs | 87.0 ms | 29.5–30.3 ms (three runs) |
| Worst of the 24 | 92.8 ms | 31.3–33.2 ms |
| Peak address space | 114 MiB | 243.5 MiB (cap 544 MiB) |
| Processor time per verdict | 88 ms | 59–61 ms, two threads |
| Processor time over 3 s idle | | 0 ms |

Held-out human clips (1,167, shards 4 and 5 of Pipecat's test set), asked
200 ms into a pause as the voice asks: 3 verdicts on the other side of 0.5,
18 in another of the voice's bands (5 newly "finished"), 95.54% correct
against 95.63%. The one multiplication kept in floats was chosen on the
development shards 0 and 1, not on these. By the exact model's measured
ratio (383 ms on lamp-4ace's fast cores against 87 ms here) the fast model
would take about 130 ms on the lamp. That is an estimate, not a
measurement.

**Two findings beside it:**

- tract 0.23.8 built in a Debian bookworm container has no fast 8-bit
  kernel: its build script's assembler probe (`.cpu
  generic+fp+simd+dotprod`) fails on binutils 2.40 and clang 14 to 16, so
  the SDOT kernel is left out without a warning. It does not touch any
  float model the lamp runs today, but anyone who tries 8-bit under tract
  will find it no faster. `.arch armv8.2-a+dotprod` assembles everywhere.
- Asked 100 ms into a pause instead of 200 ms, today's model is no worse
  at what ends a turn early: finished turns judged finished 91.5% against
  90.6%, unfinished ones judged finished 3.3% against 4.0% (held-out clips,
  digital replay). With the fast model that would put a verdict near
  230 ms. That is `ConfirmPolicy` in `crates/voice/src/activity.rs`, yours.

**Tested** on the Linux ARM64 VM, from `lampOS/`, on `1968ba608` rebased on
`lamp-voice` `40e9b1c00`: `cargo fmt --all --check` clean; `cargo clippy
--locked -p lamp-turn-end -p lamp-turn-end-wire --all-targets -- -D
warnings` clean; for `-p lamp-voice` and `-p lamp-voice-eval` clippy stops
on one warning in `lamp-body` (`run` in `crates/body/src/inspection.rs` is
never used), a crate this change does not touch, and lists no other
warning; `cargo test --locked` for `lamp-turn-end` (16 + 7 + 1 + 5, model
tests ignored), `lamp-turn-end-wire` (4 + 12), `lamp-voice-eval` (all
suites) and `lamp-voice` (744 in the library and every integration suite):
no failure; `cargo build --locked --workspace` builds. With the model and
the library present, all 4 tests of `tests/fast.rs` and all 11 of
`tests/parity.rs` pass on the VM, and the 4 of `tests/fast.rs` on macOS.

**On the lamp, as the service user, in the release directory:**

```sh
curl -fsSLO https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-aarch64-1.30.0.tgz
tar -xzf onnxruntime-linux-aarch64-1.30.0.tgz
cp onnxruntime-linux-aarch64-1.30.0/lib/libonnxruntime.so.1.30.0 .
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30                # as it is
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30 --cores fast   # held to the fast cores
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30 --fast         # the fast model
./lamp-turn-end --check-model smart-turn-v3.2-gpu.onnx --fast
```

It should show `"cores":[6,7]` on the fast runs, the second run's
`total_ms` median near 383 ms, the third's under 150 ms, and the check
exiting 0. To use it: in `crates/voice/src/coordinator/endpoint.rs`
(`EndpointRig::start`), `WorkerCommand::smart_turn_fast` in place of
`WorkerCommand::smart_turn`, for example behind an `--endpoint
smart-turn-fast` beside `smart-turn`.

**Where it is most likely to be wrong on the lamp:** the time. The
130 ms is a ratio carried over from the float model, and an 8-bit
multiplication may lose more on the A76 (half the M2's vector units) than
a float one does. If `bench --fast` is over 150 ms, try `--threads 1`
too: `lamp-name` uses the same two cores every 80 ms, and two threads can
wait on each other. Next most likely: the library not loading on the
lamp's system (it needs glibc 2.28 and `GLIBCXX_3.4.22`; the check says
so at once), and no `cpu_capacity` in the lamp's kernel (the worker then
falls back to the highest frequency; `"cores"` shows what it chose).

**Not shown:** anything on the lamp; speech through its microphone; the
fast model inside a full voice session; whether a 300 or 400 ms end suits
people.

### Task 2: try to break last night's changes to the voice (2026-10-14)

On its own branch only, as the task says: `second/voice-review`, one
commit `1d737f8e5` on `e93a828d1`. It adds tests and
`docs/voice-review-2026-10-14.md`; the one line outside a test file is
`mod review_2026_10_14;` at the end of the worker's test module in
`crates/voice/src/provider_worker.rs`. Nothing was fixed.

**Faults, each shown by a test that fails today, most serious first:**

1. **Speaks when it should not.** The note sent while someone is speaking
   ("your own ears caught your name, Lamp, just now") stays with a request
   that is then given up (judged a sound, or the microphone closed): no
   `activityEnd` is written, so the next request, by anyone and however
   much later, reaches the model with the note in front of it. Fails both
   with `kept-start` and without.
   `crates/gemini/tests/review_2026_10_14.rs`:
   `review_a_note_for_a_request_given_up_is_not_carried_into_the_next_request`.
2. **Speaks when it should not.** A name heard after a request's sound has
   ended, but before its first output, lets that request's held answer
   out (a busy room: "Hey man…" just after someone else's line).
   `crates/voice/src/provider_worker/tests/review_2026_10_14.rs`:
   `review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer`.
3. **Speaks when it should not.** The second look's own written turn
   counts as one of a new session's two protected requests, so the next
   line of other people's talk is not held.
   `crates/voice/tests/review_2026_10_14.rs`:
   `review_the_lamps_own_second_look_does_not_use_up_a_new_sessions_held_requests`.
4. **Silent to a caller, narrow.** Before a request begins, the second
   look keeps only the newest hearing, so a later faint one (0.91)
   replaces a plain one (1.0).
   `review_a_plain_hearing_just_before_a_request_is_not_lost_to_a_fainter_one_after_it`
   (same file).
5. **No effect on behaviour.** `crates/name/src/said.rs` can publish a
   hearing scored above 1, which `NameHeard::check` refuses.
   `crates/name/tests/review_2026_10_14.rs`:
   `review_a_hearing_is_never_said_with_a_score_over_one`.

**Seen by reading, not shown by a test** (the second look is switched on
only from the environment, so no test can turn it on without a seam that
was not added): an answer that opens with a spoken "..." ends as
`nothing_said` and never reaches `second_look.finished`, so that call gets
no second look; the second look counts a hearing until the turn is over and
can bring a held answer back out; it can go to a renewed session that
never heard the call; `lamp-name` dates a hearing 0.8 s early after a gap;
the worker ignores a failure to subscribe to `name/heard` without a word.

**Found sound, with six passing tests:** `try_note` cannot end the link;
the second look is taken once and does not loop; the note goes out once
per request; a name counts only for its own request. No path was found
that answers twice or loses the link.

**Tested on** an Apple M2 Max laptop (macOS, Rust 1.96.0), from `lampOS/`:
`cargo fmt --all --check` clean; `cargo clippy -p lamp-voice -p
lamp-gemini -p lamp-name --all-targets -- -D warnings` clean; `cargo build
--workspace` builds; `cargo test -p lamp-gemini` and `-p lamp-name` fail
only on their review test; `cargo test -p lamp-voice`: library 726 passed
and 5 failed, `tests/review_2026_10_14` 3 passed and 2 failed. Of the
library's 5, one is the review test; the other 4 are older
`coordinator::motor` tests that time out after 2 s when the whole library
runs at once on this laptop (21 of them in another full run, and the same
21 on `origin/lamp-voice` without the review line); one at a time, all 46
in those groups pass. Nothing was run on the lamp.

**For the first builder:** the five failing tests are the work list, in
that order; each passes once its fault is mended. Most likely to matter on
the lamp: the first, because a "Hey Lamp" said alone and judged a sound is
an everyday event there.

## Questions

1. **Answered 2026-10-14 (mended in `46403ffbd`).** **2026-10-14, task 20.** `cargo test -p lamp-voice` does not compile on
   `lamp-voice` at `d83834cb2`: `crates/voice/tests/capture_hold_worker.rs`
   line 161 calls `capture_on` with nine arguments; since the steady
   reference was wired it takes ten (the echo reference). Adding
   `lamp_audio::Reference::AsGiven` there is likely all it needs;
   `crates/voice` is yours, so it is left to you. That test's harness runs
   the real capture worker against scripted devices in a capture-clocked
   session; given a diagnostics directory (its sixth argument, `None`
   now) it would make exactly the kind of recording task 20 replays, from
   the voice's own recorder, and a test there could replay it.
2. **Answered 2026-10-14 (not needed: the replay was run on the lamp's own).** **2026-10-14, task 20.** Two of the lamp's capture-clocked recordings,
   with played voices only, to check the replay against: where can I
   fetch them? (A `.tar` on a draft release of this repository would do.)
