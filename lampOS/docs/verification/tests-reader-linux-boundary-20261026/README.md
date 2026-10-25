# Linux session-reader boundary qualification

On base `3d9722f38` plus the three-file test-only source change, one Linux
AMD64 container execution passed **14 tests, zero failed, one existing ignored**.
The ignored cross-user DAC case is unchanged and requires isolated Linux root.
The three focused cases prove late grant-12/rollback-0 failure, earlier private
writer fallback, and real mode/ACL/path refusal at the private fixture root.
The raw report and first apply error are preserved in `reader-module.stderr`.
The broader full twelve-package gate is a separate execution, pending here.

The two Rust files reconstruct from the unapplied proposal in
[100ff0c98](https://github.com/autonomous-ai/lampOS/tree/100ff0c98/lampOS/docs/verification/session-reader-boundary-source-20261026),
carried onto 3d; `source-proof.json` binds the current exact files. All 10,564
tracked source entries remained fixed through qualification. Source and ELF
hashes were independently audited. The actual frozen ELF remains outside the
build target; no binary artifact is committed.

Formatting, real Voice library compilation, strict all-target Clippy over all
twelve reverse-dependent workspace packages, and workspace build exited zero.
The receipts preserve literal command arguments and outcomes. The existing
Podman VM ran the pinned AMD64 image as UID 1000, with read-only source and
public dependency caches, no network, no extra capabilities, one compiler,
and a fresh generated target volume. The controller checked source before and
after each stage, retained complete streams, checked a 5 GiB disk reserve and
stage deadlines, and stopped only its own container. Its VM stopped only after
no other running container remained. Scratch controller stays private.

This lane used debug information level zero, optimization zero, and enabled
debug assertions/overflow checks. It establishes neither privileged DAC,
physical-lamp acceptance, a historical failure cause, ARM64 results, nor a full
Voice or dependent-package result. The focused private roots deliberately
exclude host `/` and `/tmp` ancestry; unscoped fixtures retain the original walk.
