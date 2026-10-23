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

ARM64 run **39992469455** is pending. The two external-artifact integration
cases remain intentionally ignored by ordinary CI; their separate executed
results are the historical release-pair evidence above.
