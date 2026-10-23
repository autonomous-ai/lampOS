# Combined head-touch install carry

Source `413378623759a489a5eacd6072640a3e8038da3c` on `be16c3327`.
This carries #2948 onto the merged #3269 prerequisite and adds the assigned
install preflight. [Source and output hashes](source.json).

## Behavior and regression

Install now checks the resulting product Voice, Expression and Controls
readers after its updater lock and before state migration, users, binary
links, units or pins change. It reuses the updater's conservative literal
mode and override rules. Unknown or conflicting local overrides are refused;
local drop-ins are retained. Product pins must agree with those readers.

The original two [install controls](../head-touch-install-review-20261023/README.md)
and requested removed-mute control reproduce on `f69bdea64`; all three now
pass. Their assertions are unchanged, with formatting-only edits in the
maintained tests. Additional controls preserve corrupt state, existing links
and zero host calls on refusal, then verify calibration and idempotence after
the conflicting override is removed. These are host file/admission checks,
not an observed microphone or physical privacy result.

## Host gate

Intel Mac, pinned Rust 1.96.0, checkout-specific Cargo target. From `lampOS/`:

```sh
cargo fmt --all --check
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-head-touch-install-target cargo test -q --locked --offline -p lamp-update -p lamp-controls -p lamp-setup -- --test-threads=2
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-head-touch-install-target cargo clippy -q --locked --offline -p lamp-update -p lamp-controls -p lamp-setup --all-targets -- -D warnings
```

All commands exit zero. [Full tests](host-tests.txt): **1,062 passed, 0 failed,
6 ignored**, all 19 targets complete, including installer **87/0** and rounds
**151/0**. Two ignored custom harnesses require Linux tracing; none of the new
controls is ignored. [Strict Clippy](host-clippy.txt) emitted no diagnostics.
The earlier [focused controls](focused-controls.txt) were **3/0**.

The first full-gate attempt failed to compile the new retry test because it
referenced a helper scoped inside another test. That [failure](test-compile-failure.txt)
is retained. The test now reads the file directly; the full corrected run
above completed once. No failed runtime result was rerun or waived.

The full tests ran on the `09f8e7412` base. Before Clippy/push, the carry was
rebased onto `be16c3327`, whose sole delta is `docs/build-rows.md`, unreferenced
by the crates. Both the committed code and full five-file repair diff were
verified identical across that rebase; Mac formatting was checked again.
The [carry range-diff](carry-range-diff.txt) preserves the original activation
commit and adapts only the generic voice census to the already-landed clock
group while keeping generic head touch excluded.

## Remaining gates and limits

Linux formatting, PR ARM64 and Linux Clippy remain to be recorded on the
pushed head. The existing audible-cue condition remains with its assigned
owner: this head still has the −35 dBFS code default. Documentation separates
Fifth's attended hand trial from Thirteen's injected-input fault/readiness
check. No lamp was accessed, no ARM64 binary linked on this Mac, and no
exact-new-build acoustic latency or physical acceptance is claimed.
