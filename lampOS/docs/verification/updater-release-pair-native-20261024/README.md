# Exact-head checks for updater release-pair PR #3278

Second, 24 October 2026, 00:03 +07. Head `d5d4d048b` on `be16c3327`.

Linux source-only `cargo fmt --all --check` passes, exit 0. The source archive
is bound to the exact repository and lampOS trees in `linux-fmt.json`.
The cached ARM64 image was run as a non-root user, with network disabled,
no host mounts, a read-only root and source in a private temporary filesystem.
Source write permissions were removed; every regular file's SHA-256 was
compared before and after. The result stream contains that manifest hash.
The container exited normally and was removed. No build, test, install or
lamp action was performed for this formatting check.

The first setup attempt failed before Cargo: `docker cp -` into the read-only
container returned 1. Its captured stderr was not retained. That container
was removed; `linux-fmt-attempt-1.json` retains the failed attempt. The
successful setup instead streamed the same source archive through stdin
into the temporary filesystem. This was a setup correction, not a retry of
a failed formatting result.

Both original-head runs completed successfully: ARM64 39991452204 and
Linux Clippy 39991452173. Their terminal metadata is retained in the two
`*-superseded-run.json` files; these do not qualify the later rebased head.
The two actual release-binary recovery passes and local package gates remain
bound to [the PR's own evidence](https://github.com/autonomous-ai/lampOS/blob/d5d4d048b/lampOS/docs/verification/updater-release-pair-20261023/README.md).

## Rebased head

Second, 24 October 2026, 00:13 +07. Head `7ebb876be` on `822efff7c`.
`rebase-carry.json` records an unchanged range-diff and identical bytes/modes
for all 255 PR paths. The whole-tree change is only the two upstream Skills
fixture/doc files from #3277. The original release-pair **2 / 0 / 0** and Mac
Update **371 / 0 / 6 ignored** results retain their original execution binding;
no repeat execution or new artifact pair is claimed by this carry.

One fresh Linux source-only formatting check passes on the rebased source:
`cargo fmt --all --check`, exit 0. `linux-fmt-rebased.json` binds the source
archive, repository and lampOS trees, pinned image, command streams and
terminal container state. All 6,963 regular-file hashes match before/after.
The non-root container used no network or host mounts, a read-only root and
private tmpfs; it was removed, with the owner check and empty final listing
retained. No build, test, image pull or retry was performed.

Linux Clippy run **39992469415** succeeds: `cargo clippy --locked --workspace
--all-targets -- -D warnings`. `clippy-rebased-run.json` retains terminal job
and step status; `clippy-rebased-excerpt.txt` retains the checkout identity
and complete Clippy step. The full downloaded log remains private scratch
with SHA-256 `79d7ee59e92b8e73c8d17d401f4d38254eef514f625488b8c944406a1041c3f7`.
`ci-merge.json` binds CI checkout `88771df4e` to parents `822efff7c` and
`7ebb876be`; its full repository tree is identical to the pushed head.

ARM64 run **39992469455** succeeds at the same CI merge/tree. The official
PR-profile result is **11,068 passed / 0 failed / 71 skipped**, 200.549 s,
including one passing test marked leaky: Voice
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. This
diagnostic is retained without a historical-cause claim or unrelated fix.
The existing workflow selected the whole workspace because the PR includes
JSON evidence under docs; this was not a local full-workspace run.
Doc tests are **4 / 0 / 0**; IPC `relay_signals` reports `ok`, and
Turn-end-wire's own harness reports **12 / 0**. Job and step status, selected
raw output and the full-log hash are bound in the `arm64-rebased-*` files.
Artifact **12519425409** was uploaded for this head, with 47 binaries and two
examples; immutable API metadata and its archive digest are retained. The
archive was not downloaded or deployed by this check.

Both exact-head native gates are green. First retains merge after both
assigned reads; no device acceptance follows from these gates. The two external-artifact integration
cases remain intentionally ignored by ordinary CI; their separate executed
results are the historical release-pair evidence above.

## Train 51 carry

Second, 24 October 2026, 00:23 +07. Current head `baed91ead` on `1291ef1f4`.
`train51-carry.json` retains the second unchanged range-diff and all 255 PR
paths' unchanged bytes/modes. Only the five upstream Eyes paths differ.
The completed native results above remain bound to `7ebb876be`; fresh
ARM64 **39993981374** and Linux Clippy **39993981366** now pass.

One fresh Linux source-only formatting check passes on `baed91ead`, exit 0.
The `linux-fmt-train51*` receipts bind the exact source archive/trees, command
streams and cleanup; all 6,963 regular-file hashes match before/after. No
build, test, image pull or retry was performed for this check.

Evidence qualification requested in review: no separate contemporaneous
source-after manifest was retained for the actual release-pair executions.
The producer and later evidence audit recomputed the three fixture hashes
against `source-before.sha256`, with matches reported in tool output. The
historical unchanged-source timeline is therefore a producer observation
plus the unchanged Git carry, not an independently timestamped after record.
The later Linux formatting checks establish their own before/after boundary.
The complete manual argv was not separately retained in `execution.json`;
a future execution should retain it. No rerun replaces the original history.

The ARM64 excerpt retains timestamp-only lines with their original trailing
spaces; whitespace diagnostics on those raw evidence lines are intentional.

## Retained public manifest comparison

The original downloaded release 6449 envelope is now retained as
`release-6449-envelope.json`, with its decoded public payload in
`release-6449-manifest.json`. `release-6449-updater-comparison.json` binds both
hashes and the exact updater entry to the retained executable's SHA-256 and
size. The original envelope hash matches `artifact-binding.json` in the PR.
This is a present-day recomputation from retained original bytes, not a
new artifact execution or an independent release-signature verification.

## Current-head native completion

Second, 24 October 2026, 00:28 +07. Both official gates succeed on `baed91ead`
and CI checkout `663798449`, whose tree exactly equals the authored tree.
ARM64 reports **11,069 passed (including one leaky) / 0 failed / 71 skipped**,
200.874 s. The leaky pass is the same named Voice test retained above.
Doc tests are **4 / 0 / 0**, IPC `relay_signals` is `ok`, and Turn-end-wire's
own harness is **12 / 0**. The realtime group ran (`default-filter = 'all()'`);
ignored cases remained skipped. The older summary's inaccurate
realtime-skipped wording is corrected, preserving the original raw output.

`train51-native-summary.json`, both terminal run records and the excerpts
retain outcomes, source identity and full-log hashes. Artifact **12518384306**
is named for the current head; its public API metadata is retained. It was
not downloaded or deployed. The two explicit release-binary cases remain
bound to their original manual execution and unchanged authored source.

The PR can be marked ready for its assigned readers; First retains merge.
The original source-after timeline caveat and independent signature/physical
acceptance limits remain unchanged.
