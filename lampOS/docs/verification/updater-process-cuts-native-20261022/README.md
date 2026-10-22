# Updater process cuts: native checkpoint, 22 October 2026

This checkpoint preserves the successful earlier phase of [PR #3012](https://github.com/autonomous-ai/lampOS/pull/3012)
for [#2810](https://github.com/autonomous-ai/lampOS/issues/2810). It does **not** qualify the latest rebased PR head or close
the physical-unplug work.

[Official ARM64 run 39774177876](https://github.com/autonomous-ai/lampOS/actions/runs/39774177876)
passed at head `33a43cdd245146bf674127b4792f28b3576de879`, tested as merge
`d4668ab93d1fb89d8a535741b28da0b64ca3a268` with base
`8e274b5e394b09ef5e5f18cb0d2382fbaddec95a`:

- Nextest: **10,451 passed / 0 failed / 67 skipped**, **179.036 s**. One passing
  test was marked LEAK: `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
- The updater process-cut checker explicitly passed in **3.755 s**. It is one
  checker covering seven cuts; those seven cases are not added to the test total.
- Separately: **4 doctests**, **12 process-harness tests**, and `relay_signals: ok`.
  Captured nested test stdout is excluded from all additional counts.
- [x86 Clippy 39774177794](https://github.com/autonomous-ai/lampOS/actions/runs/39774177794)
  passed on that same old head. This checkpoint verifies its run/step metadata;
  it does not claim an independent audit of a Clippy log.

The ARM64 log retains eight Cargo example-output collision warnings and a
Node action-runtime deprecation annotation. [native.json](native.json) records
the exact warning and leak rows, counts, source identity, and raw-input hashes.

Artifact **12425151983** was fully downloaded and rechecked offline: **50 ZIP
members**, **49 checksummed payloads** (47 binaries and two examples). All
member hashes match `SHA256SUMS`; archive digest matches the recorded API digest:
`df8de6db01be5d1424567818c43b7f0adaf87b3bc1e28a0802ce95a56497b6e1`.
The retained updater is ELF64 little-endian AArch64, SHA-256
`a42dbaade44bc19a65addcb18bd09d1dbdf764c55d571f0e05551c6200dce535`. It was not executed or copied into this proof. A PR merge
artifact is not a signed release or production-publication input.

The later, **unpublished** local head `38aab2919d47b42a8b27c95448ca8ab7201b8923` on
`d7492ca743141d9c8afbefbd390407e1e58132aa` passed Mac fmt (**5.160 s**), strict all-target
Update Clippy (**17.400 s**), and Update tests (**345.358 s**):
**306 passed / 0 failed / 3 ignored / 0 filtered**. [mac.json](mac.json) preserves
all 35 unchanged source hashes and the equal (`=`) range comparison. The raw
receipt's `base` field names the local head; this export labels that distinction.
The Linux-only process checker is one of the Mac ignores.

At this checkpoint, the public head is `53911e246558cfdfd7f7f371b51afdd235fdc5ea` on
`6f1123f69b33e352d5ba5e149a47fc68ea079d7a`. Its patch range remains equal to the old
qualified commit, with later integration changes. Fresh
[ARM64 39778575538](https://github.com/autonomous-ai/lampOS/actions/runs/39778575538) and
[Clippy 39778575540](https://github.com/autonomous-ai/lampOS/actions/runs/39778575540) are recorded as **running,
not qualified**. Their eventual result belongs in a separate receipt.

The [initial cut/control proof](https://github.com/autonomous-ai/lampOS/tree/33a43cdd245146bf674127b4792f28b3576de879/lampOS/docs/verification/updater-process-cuts-20261022) retains prior failures and the limits
of syscall-stop recovery. The [attended-unplug plan](https://github.com/autonomous-ai/lampOS/blob/33a43cdd245146bf674127b4792f28b3576de879/lampOS/docs/verification/updater-attended-unplug-plan-20261022.md) remains unexecuted;
physical interruption requires the assigned person and approved lamp access.
The [#1812 signed-release exercise](https://github.com/autonomous-ai/lampOS/issues/1812) is also undone and still
needs owner/key/publication and device prerequisites. No physical power-loss,
flash durability, installed-daemon handover, or real signed-release result is
claimed here.

These are compact normalized records, not copies of private raw logs or
receipts. [SHA256SUMS](SHA256SUMS) binds all public files except itself. No raw logs,
artifact archive, updater binary, keys, or personal host paths are published.

## Later checkpoint, 19:48 +07

Intermediate head `53911e246558cfdfd7f7f371b51afdd235fdc5ea` passed ARM64
39778575538 and Clippy39778575540, tested as merge
`8e5b6f402cefb75e3babb898eeaff1fb85c4618b` with base6f1123f69.
[rebase-539.json](rebase-539.json) preserves **10,460 passed / 0 failed /
67 skipped**, one retained leaky Voice pass, explicit checker PASS3.678s,
4 doctests, 12 process tests and relay success. Artifact12427315012 was
recorded from API metadata only; this replacement archive was not downloaded
or independently verified. The earlier authored archive verification remains
separate.

The latest-base guard then advanced to `135f14957e4a8560b3f72fa785ed62f605f800dc`,
a Skills-only upstream change. Equal-range replacement
`1030fb8ab5b05e25099e1946f83e4ab319919fd3` is now public. All bound Update source
files still match the completed Mac receipt. Its fresh ARM64 39780978501 and
Clippy39780978544 are running; neither older green qualifies this replacement.
Physical unplug and #1812 remain unexecuted, awaiting First/owner prerequisites.

## Later checkpoint, 20:32 +07

Two more unchanged-patch heads completed both native ARM64 and Clippy:

| Head | ARM64 run | Clippy run | Passed / failed / skipped | Checker seconds |
| --- | --- | --- | --- | ---: |
| `1030fb8ab` | 39780978501 | 39780978544 | 10460 / 0 / 67 | 3.888 |
| `d369b1d0c` | 39782710438 | 39782710190 | 10466 / 0 / 67 | 3.581 |

[rebase-1030.json](rebase-1030.json) and [rebase-d369.json](rebase-d369.json)
retain each exact head, tested merge/base, counts, leaky Voice pass, four
standalone doctests, twelve process tests and relay result. For d369, CI tested
merge736468b against **a11c53523**, newer than its branch base69f786892; those
identities are deliberately distinguished. Both artifact records are API
metadata only, not independently verified downloads. Captured nested test
stdout is not added to the counts.

The fifth head, `6c4088120df275d74341274cf91db0e4d3414e13`, contains
`b1b9174f84a916783918e9eac295b64de086ad81`, the latest pre-push integration.
Clippy39785883405 passed; ARM6439785883299 is still running at this checkpoint.
Reviewer-lead's unchanged-patch pre-read found no P1/P2. A merge slot was
requested because integration advanced during each earlier successful run.

Separately, [five ARM64 observer controls](../updater-observer-controls-20261022/README.md)
passed using the verified authored artifact's `--help` only. This is debugger
preparation, not an installed self-switch or physical power-cut result. Its
Podman VM was stopped after capturing the results. Actual lamp/person/trial
inputs and #1812 publication prerequisites remain with First/owner.

## Completed required checks, 20:44 +07

[rebase-6c40.json](rebase-6c40.json) records head `6c4088120` tested as merge
`b26032a` with base `b1b9174f8`: **10,477 passed / 0 failed / 67 skipped**,
one leaky Voice pass, explicit process-cut checker PASS **4.922 s**, four
doctests, twelve process tests and relay success. The ARM64 job119848843550
and Clippy job119848844430 are both SUCCESS. Workflow39785883299's aggregate
status remained in_progress even though all jobs were success or skipped;
this receipt claims the completed required checks, not a completed aggregate.
Artifact12429233152 is API metadata only, not an independently verified download.

The latest integration guard then found `f481f7454`, so the PR remains draft.
First reports that integration needs #3044's fmt correction. Reviewer-lead's
merge order puts that and #3042 ahead of #3012. The next rebase is deliberately
waiting for those merges, rather than running a gate on the known fmt-broken
base. This is a merge-order dependency, not a failure in the updater patch.

## Corrected-base replacement, 21:24 +07

The clean local branch was rebased onto `2297f0f3d` as `a0cc8b9c3` after
reading the current builder protocol. [mac-2297.json](mac-2297.json) records
strict all-target Update Clippy (**33.002 s**) and full Update tests
(**247.880 s**): **306 passed / 0 failed / 3 ignored / 0 filtered**.
The 35 bound inputs were unchanged before/after these commands and from the
prior completed local gate. Global fmt still failed only in the upstream Voice
own_echo.rs:317 test; the receipt retains that failure instead of hiding it.

After #3044 merged, the equal-range final rebase became `d4d5e5a66` on
`c6d50559a`. All 35 tested inputs remain identical, and global fmt passed on
that final head. That own branch is pushed. Its new ARM64 run39795365169 and
Clippy run39795365207 are pending, so no earlier native green qualifies it.
The PR remains draft pending its replacement checks and latest-base guard.

## Sixth native qualification, 21:45 +07

[rebase-d4.json](rebase-d4.json) records head `d4d5e5a66`, merge `a4e570c`,
base `c6d50559a`: ARM64 39795365169 and Clippy39795365207 both passed.
Nextest: **10,491 passed / 0 failed / 67 skipped**, one leaky Voice pass,
checker PASS **3.967 s**, plus four doctests, twelve process tests and relay.
The artifact is now fully verified, including all49 payloads and its API
archive digest, not merely metadata. Its updater hash differs from authored33a.
The [actual CLI proof](../updater-cli-exec-20261022/README.md) therefore tests
both artifact identities separately and retains their fixture limitations.

Integration advanced to `b722fd212`, including an Update installer change.
The next equal-range local head is `97ca7b322`; fresh Update local gates are
running because that source changed. No old green qualifies that replacement.
A coordinated merge window was requested from First/Reviewer-lead so the
latest-base guard can pass once the replacement checks complete.
