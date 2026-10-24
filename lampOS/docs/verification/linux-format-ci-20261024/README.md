# Explicit Linux formatting CI gate

**Disposition:** duplicate #3375 was closed in favor of Third’s earlier claim and #3373. The own branch and all local evidence remain; no competing merge is requested. Cancellation was requested only for its still-running exact-head ARM job. These receipts retain the validation performed before that ownership correction.

PR #3375, source head `e36f065eb242ad904c21a30243ec3024f16b2fdc`, base `6902cf8e203d165f63e73059d2747a9ec7002642`. Two ordinary workflows now run mandatory `cargo +1.96.0 fmt --all --check` from `lampOS/` after Rust setup. The x86 installer adds rustfmt. English and Vietnamese notes are in the PR.

Local validation: **2 YAML parses**, **4 extracted shell bodies pass bash -n**, both new steps are unconditional without an error override, and removing only those steps plus the x86 rustfmt component restores every base YAML field. `git diff --check` passes. `receipt.json` records the structural checks; its preparation flags describe the time before commit. `final-head.json` separately binds all four validated source-file digests to the pushed commit.

Mac's exact pinned Rustup command passes, exit 0. The initial offline Linux command exits 101 before formatting because this cached image has direct Cargo rather than Rustup (`+1.96.0` is a Rustup selector). This failure is retained. Cargo 1.96.0 and rustfmt 1.9.0-stable were verified in image `eee1c1827d71132efdd4bfd72bf5407d7df568b0cf1d14911096745df03cfb6c`; its direct `cargo fmt --all --check` passes, exit 0. Source is read-only in that offline rootless Linux AMD64 container, without a build-cache mount; source-status/diff and cleanup checks pass. Mac took 12.629 seconds and Linux direct 14.499 seconds; these are host formatting runtimes, not lamp interaction measurements.

The workflows install Rustup explicitly, so actual ordinary ARM64/Clippy CI must establish the new selector step there. No full-crate, native ARM64, Actions runtime or device result is claimed by this local packet. PR remains draft pending its required CI; source review requested on that exact head. No runtime code, job identifier, trigger, matrix, cache or publication configuration was otherwise changed.

Raw empty success streams and the failed selector stream are preserved. No Python helper or binary is committed. `files.json` hashes every other file in this packet.

The two extracted Rust-setup shell bodies retain the exact YAML scalar bytes, including their terminal blank line; publication whitespace checking excludes only those two raw extracts and raw logs. Source-workflow `git diff --check` passed separately.
