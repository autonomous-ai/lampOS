Part of #1569

Historical validation ran on parent 22b753df5db6434d21455f40f67926e0ecee9d91
with the exact four-file delta later committed as 4e96727d7b. That commit was
pushed and rebased cleanly onto 4270083a2, producing
fb8d2f71b836266df07bf1b658e40c0aa13804f3; the exact range-diff is unchanged (=).
The four authored paths did not change upstream. These are historical 22b
validation receipts, not validation of the new 427/fb8 composition. New-base local
gates and native Linux/ARM64 CI remain pending; this is not a ready claim.

The updater's built-in executable fixture now waits for its first output byte,
then both child exit and stdout EOF. A five-second startup watchdog covers the
first byte; one absolute two-second finish watchdog starts there and later bytes
do not renew it. This explicitly changes the test fixture timing boundary. The
single-threaded nonblocking collector preserves exact output/error handling and
fd flags, with the existing direct-child kill/wait guard.

Test-only: exact old/new script bytes, mode/link/state prechecks, complete expected
payload, exit-success check, and all confirm/put-back/recovery assertions remain.
Product code, APIs and product deadlines are unchanged.

Validation on x86 macOS: pinned fmt check and strict twelve-package Clippy passed;
workspace build exited 0. Its raw provenance rejection/false terminal flag remains
retained separately; a reviewed read-only repair established all four canonical
copy aliases from exact prior unit/source/profile/features/output identities and
all final output hashes/modes. This repairs provenance, not test outcomes.

The default-parallel full run remains FAILED: 7,300 passed / 10 failed / 52 ignored
(Voice 4,000/4/21; eleven readers 3,300/6/31). Update Round passed 167/167, including
all ten fixture controls on the final formatted source. All ten other full-run
failures are named in the qualification evidence. On the exact matched parent,
each passed one standalone observation (10/10). The original frozen current
witnesses then each passed exactly three observations (30/30); no retries or
rebuilds. This is the recorded standalone classification, not a full-green claim
or proof of historical cause. Memory used the exact captured reader-built children
at their original paths, then restored workspace bytes/modes; all guards and owned
process cleanup passed.

The separate controlled 2,200 ms pre-output gate showed the old after-spawn helper
time out while the gate was still held, whereas the new helper completed with the
same scheduled gate. The old control's PASS means it observed the expected old
timeout. The earlier original-case timeout diagnostic passed with its probe unused:
no historical cause finding and no relabelling of the earlier two-of-three HOLD.

Evidence: [qualification and original failed-run records](https://github.com/autonomous-ai/lampOS/blob/lamp-tests/lampOS/docs/verification/tests-update-parent-qualification-20261024/README.md), including the manifest, primary raw streams, original failure excerpts, executable/context bindings and restoration receipts. Native Linux ARM64 build/test CI is pending the PR; no ARM64 build was
run on the Mac and no device action was taken.

Self-review: child/stdout ownership, cancellation and bounded cleanup preserve the exact status/payload and update-state assertions. No people data, cache, switch, topic or message-field changes.
