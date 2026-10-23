# Frozen phone fixture: Linux formatting

`cargo fmt --all --check` passed once (exit 0) on Linux x86-64 for the
frozen phone fixture candidate: base
`e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc` plus exactly two source snapshots.
This input is not a committed head. A later commit needs a complete source-tree
binding before this receipt can qualify it.

The replacement files were `lampOS/crates/voice/src/messages.rs` (SHA-256
`e0b3d7b167ff6c8d5c805cd4a990c2e8ed2956b31c0eb0eb8ad9fb0881f3d0de`)
and `lampOS/docs/messages.md` (SHA-256
`c45dd64a5c938962ce270619022ddca72af7dc86f520318a51737b668349a0d3`).
Their original base bytes and Git modes were checked before replacement.
The final archive matched all 6,649 frozen lampOS paths, byte counts, SHA-256
values and Git modes. The complete-repository candidate aggregate from the
original frozen preparation is
`5dd15400b0c4a66eed69f9f02de03e574cbacf429e48c738d60a78d93349aa97`;
this container's input scope was the full lampOS subtree.

The check used the pinned existing Linux amd64 image with Rust/Cargo 1.96.0
and rustfmt 1.9.0-stable. It ran from `/tmp/source/lampOS`, with a read-only
root filesystem, private tmpfs, network disabled, no image pull, no host mounts
and no shared Cargo target. No source edit, compilation, test, ARM build,
device access or workflow rerun was performed.

The container command started at `2026-10-23T05:00:20.779321+00:00` and finished at
`2026-10-23T05:00:37.585731+00:00`. Its 16.807 seconds include
container startup, toolchain identity checks, extraction, rustfmt and source
hashing; this is not an isolated rustfmt duration.

The final 218,234,880-byte input archive had SHA-256
`58156397b3550fd58852df6cced7df3982bb285b3c3ba2d1f6f5bf9b34139d76`. The archive remained unchanged after the run.
The recorded Git source identity also matched before/after. In-container
manifests of all regular-file bytes and symlink targets were byte-identical,
with SHA-256 `a4294080e094b7bc80ddd833d28010e3d9a64851ce04ca0f5f5e2ed24bef88f4`. Their construction and comparison commands
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
