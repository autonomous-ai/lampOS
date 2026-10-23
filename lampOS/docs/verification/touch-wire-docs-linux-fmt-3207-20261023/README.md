# PR #3207: exact-head Linux formatting

`cargo fmt --all --check` passed once (exit 0) on Linux x86-64 for exact
head `61a195a186c94de88d4039d1f963961c4c6c9685`, parent
`51186d5ea722933e13f0ff910dc56288e187945c`. This fulfills the
[explicit formatting request](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6357444592)
for [PR #3207](https://github.com/autonomous-ai/lampOS/pull/3207).

The input archive contained all 6,656 tracked lampOS entries from that immutable
commit. There was no worktree overlay. The authored change consists of four
added lines in `lampOS/docs/msgs.md`; all five current PR comments and the
[carry READ-OK](https://github.com/autonomous-ai/lampOS/pull/3207#issuecomment-6357483216)
were read before execution. The prior carry review and next-train restriction
are not changed by this formatting receipt.

The check used the pinned existing Linux amd64 image with Rust/Cargo 1.96.0
and rustfmt 1.9.0-stable. It ran from `/tmp/source/lampOS`, with a read-only
root filesystem, private tmpfs, network disabled, no image pull, no host mounts
and no shared Cargo target. No source edit, compilation, test, ARM build,
device access or workflow rerun was performed.

The container command started at `2026-10-23T05:00:48.685514+00:00` and finished at
`2026-10-23T05:01:04.426268+00:00`. Its 15.741 seconds include
container startup, toolchain identity checks, extraction, rustfmt and source
hashing; this is not an isolated rustfmt duration.

The final 210,309,120-byte input archive had SHA-256
`ca06ed8aa13986d7811656bf47080516a0889ed9422dab9b52afde828275af4a`. The archive remained unchanged after the run.
The recorded Git source identity also matched before/after. In-container
manifests of all regular-file bytes and symlink targets were byte-identical,
with SHA-256 `82166a7f989e6f20aaa51936fa33c529888fb403287b01e2ff9dc21616dc13d5`. Their construction and comparison commands
are retained; their complete per-file contents were transient. This does not
claim inode metadata coverage.

`commands.json` retains the exact commands and terminal exits.
`command-streams.json` contains every command's complete stdout/stderr as
lossless UTF-8 strings with byte counts and SHA-256, except image inspection.
`image-provenance.json` publishes only its minimal identity/platform/UID
projection and the raw external response's hash; no author, account, label or
build-history metadata is included. Other streams are unmodified.
`source.json` binds the source recipe and hashes; `result.json` records the
terminal check. Archives, source exports, Python runners, binaries and full
source maps are not bundled. `manifest.json` hashes every publication file
except itself.

This formatting-only result does not establish behavioral correctness, test
success, readiness or qualification of another source. Both formatting checks
were executed once, sequentially; no failed execution was retried. The obsolete
#3190 `0f2e6439e` formatting plan was not executed.
