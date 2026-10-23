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

Official ARM64 run 39991452204 and Linux Clippy run 39991452173 are still
pending. Their terminal source/checkout/results will be retained separately.
The two actual release-binary recovery passes and local package gates remain
bound to [the PR's own evidence](https://github.com/autonomous-ai/lampOS/blob/d5d4d048b/lampOS/docs/verification/updater-release-pair-20261023/README.md).
