# Music rebase composition — 17fdf

This is a static preservation proof for PR #1757 at `d96f484ec0567ecd66819cb45bd44f5c39961118`, based on `17fdfda1d757324b8437d997304e855e8ecbf9dd`. It does not qualify the new head for merge. Fresh five-crate host validation is complete below. Native qualification for this head is pending.

The previous candidate is `bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0` on `fc800db6f839cf006098f73ee84942572acbccfa`. The original reviewed head is `0a878cd4d9d44867b2971e70c118244f0c21ad38` on `f349d63c7935dd16decc0badd4dd4e18c10094eb`; its parent was derived from local Git. [identity.json](identity.json) pins all six commits and trees, exact patch and file hashes, the comparison method, and its limits.

## Composition checked

The only reported conflict was `lampOS/crates/voice/src/second_provider/worker.rs`. Its complete upstream `LAMP_SECOND_SEEMS` setup remains, followed by the unchanged Music `requesting_audio(boot, runner)` setup. Both builders set distinct fields before a turn. The worker still reads current face count and applies the one-face filter; the entire `seems_told.rs` module matches the new base. The existing known-person note test retains its Written, held-gap and room-scope exclusions.

The seven Voice files changed on both sides preserve the union of their independent edits: `coordinator.rs`, `lib.rs`, `provider_worker.rs`, `second_provider.rs`, `second_provider/tests.rs`, `second_provider/worker.rs`, and `tools.rs`. Six match that union byte for byte. The worker matches after explicitly accounting for the requested-audio builder following the seems-note block. Eighteen Music-only Voice files match the previous candidate, and fourteen upstream-only Voice files match the new base, including privacy-gap support and regressions.

Static inspection found no P1/P2 in this composition. Current-main privacy/cancellation ordering and the different-voice Heeding fix remain. Music's raw-spoken admission, request/scope binding, Policy hold, private-offer separation, NEWS extraction and final-send checks remain. No combined seems-note/Music runtime test is claimed by this review.

## Patch and proof preservation

The old and new Music patches touch the same 89 paths. Seventy-eight files are byte-identical to the previous candidate; eleven incorporate newer-base context. All 34 verification files are byte-identical to both the previous candidate and the original reviewed head. Their historical successes and failures remain historical evidence and do not qualify this rebase.

[range-diff.txt](range-diff.txt) compares the previous candidate with this rebase. [from-reviewed-range-diff.txt](from-reviewed-range-diff.txt) compares the original reviewed patch with this head. Both report a changed (`!`) range-diff; this is not an equal-patch claim. The immediate rebase comparison includes the explicit builder placement and upstream switches-document context. The longer original-review comparison records earlier upstream integrations as provenance; this task did not re-review that entire history or every non-Voice change.

## Hashes and limits

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `identity.json` | 54258 | `960ee114c4750b99d4a3cee2b77b0339c5bbdda2a703c2d2dc6c77b1a75d93af` |
| `range-diff.txt` | 4936 | `c0ed7e3d8fab5c6c5df76389c10682e1501a6fa7aeaf13528ddc2e4203faa12c` |
| `from-reviewed-range-diff.txt` | 14637 | `3b20f475627d54832c19edb2cbe317190a3d1bf6de5006a6d55f5a87b83dabc7` |

The resolved worker is SHA-256 `5a01df4b3a4b9593638b54b9aac16d7b68924e0751bbd5d888bfab8d5809390a`. Source hashes come from the pinned commit, independently of the running gate. This preparation used read-only local Git and static comparisons only: no compilation, tests, provider/device activity, network access, source changes, or Git metadata changes. Fresh gate results and actual-head native evidence must be added separately before a readiness claim.

## Completed Mac gate and native runs

Rust 1.96.0, run from `lampOS`, locked/offline Cargo, two build jobs and serial
tests. [mac-gate.json](mac-gate.json) retains all exact commands, output summaries,
raw-log hashes and before/after source hashes. All captured sources stayed fixed.

- `cargo +1.96.0 fmt --all --check`: exit **0**, **4.989 s**.
- `cargo +1.96.0 clippy --locked --offline -p lamp-msgs -p lamp-actions -p lamp-skills -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings`: exit **0**, **36.034 s**.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-msgs -p lamp-actions -p lamp-skills -p lamp-voice -p lamp-voice-eval -- --test-threads=1`: **4854 passed / 0 failed / 14 ignored / 0 filtered out**, exit **0**, **713.929 s**.

Only the five changed crates were tested locally. No full local workspace gate
or local ARM64 build was run. Before push, a Git merge-tree check against the
subsequent `a52357bb0` base succeeded without conflicts. That static merge check
is not an execution against that later base; PR CI validates its tested merge.

Pushed head: `d96f484ec0567ecd66819cb45bd44f5c39961118`.
[ARM64 39713220661](https://github.com/autonomous-ai/lampOS/actions/runs/39713220661)
and [Linux Clippy 39713220653](https://github.com/autonomous-ai/lampOS/actions/runs/39713220653)
are the new-head runs. Ready for lamp-voice: **no, native qualification pending**.
First owns the merge. Historical successes and failures above remain tied to
their original sources.

The two range-diff files preserve Git output byte-for-byte, including its padded
blank context lines. Those two raw-output lines trigger `git diff --check`;
ordinary whitespace checks exclude only these raw evidence files. All other
added files pass the normal check, and the source gate passed before push.
