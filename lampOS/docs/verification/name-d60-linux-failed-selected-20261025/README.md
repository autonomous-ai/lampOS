# Name d60 Linux failed gate

All five stages were observed. Formatting, strict workspace/all-target Clippy, workspace build and full Update passed. Full Voice failed: 4207 passed, 4 failed, 24 ignored. Update: 398 passed, 0 failed, 4 ignored. All six required controls passed once, all 46 local workspace members have bound fresh compilation evidence, and source stability, cleanup and target release verified. Overall exit remains 101.

The four failures are in existing capture_hold_worker integration cases: another microphone (ReferenceLate at line 472), four-block microphone hold (ReferenceBacklog at 472), stop-word blocks (QueueTiming at 472) and render 5 ms after capture (24 of 200 blocks over 1 ms at 1321). The entire fixture is byte-exact fc31; this establishes source parity, not cause. Tests was notified on the Queue and PR. No retry-pass or timing-threshold change occurred.

Commands: cargo fmt --all --check; cargo clippy -q --offline --locked --workspace --all-targets --message-format=json-render-diagnostics -- -D warnings; cargo build -q --offline --locked --workspace --message-format=json-render-diagnostics; cargo test --offline --locked --no-fail-fast -p lamp-voice -- --format pretty --test-threads=1; cargo test --offline --locked --no-fail-fast -p lamp-update -- --format pretty --test-threads=1.

Private manifest SHA-256: 779f7daa910c150f398ebb5a9c0508f1cb310bd7e121021bdffd375ec585aa57. Selected summary SHA-256: bb3d03f7e43bc528be8c084a957418169f199b221ab0df9a918f5ff59ad0e96f. Raw logs, source maps, compiler data and a non-executable private post-terminal driver copy remain private. Its initial failed-execution hash was not observed; the later capture does not close that gap. No diagnostic test was executed.

The public NoPause baseline49 selects no Voice case, so it supplies no observation or pass for these four boundaries. Mac, final parent composition, native ARM64/Linux Clippy, privacy and physical acceptance remain separate. The Common dependency and child order holds remain; no ready or activation claim follows. Readers have not independently recounted private evidence or retrieved this unpublished source candidate.
