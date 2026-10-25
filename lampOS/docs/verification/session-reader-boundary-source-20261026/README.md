# Session-reader rollback boundary source proposal

Ready for lamp-voice: no

Part of #1806; follow-up to #3394 comments 6386376127 and 6402011440.
This is an **unapplied source proposal**, published so a supported Linux seat
can qualify the concrete candidate. It changes no compiled crate on this log
branch. The implementation remains on its own local Tests branch until its
gates pass. No compiler, Linux test, fixture control or device action has run.
Formatting and diff checks pass on the source candidate; source review found
no remaining P1/P2. These are not runtime results.

Apply `candidate.patch` with `git apply --unidiff-zero` on base `a6ccb1aa3`; `source-proof.json` binds all three
original and proposed files. The patch retains the original late-rollback
assertions and additionally requires actual grant-12 and rollback-0 hits.
It records the first apply error, including error kind and OS error, before
checking the actual report fields separately. Two additional controls check
an earlier verified-private fallback and the private root's mode, ACL and
path-prefix refusals. The earlier writer control retains its original
500 ms finish snapshot and then observes the existing completion event for
its data assertions; it does not extend the production finish deadline.

Only these three Linux fixtures enter a thread-local, pinned private root.
The original common ancestor/openat/no-follow/owner/mode/ACL walk runs at and
below that root. **Host `/` and `/tmp` ancestry are deliberately outside these
focused fixtures' scope.** Every unscoped security fixture still uses the
original `/` root. The scope checks its named/pinned root identity, refuses
an escaped prefix and nesting, cannot move between threads and restores on
unwind. No chroot capability or global environment mutation is required.
All new implementation hooks are `cfg(test)` and Linux-only; no product
privacy validation, grant/rollback action or report policy is changed.

Qualification must run the actual Linux code and retain the three case
outcomes and stage/error/report evidence, then complete the affected-crate
gates and exact-current-head native ARM64/Linux checks before readiness.
No fixed sleep, ignored case, relaxed assertion or retry-until-green is part
of this candidate. Any compact debug-info build must retain assertions and
report its distinct profile; it is not a default-profile comparison.

Original ARM64 40224140937 attempt 1 remains failed: 10,003 passed, 1 failed,
106 skipped and 1,304 selected cases unrun. Its successful retry does not
establish why the original grant stopped early. The older train-94 failure
also remains recorded on #3394. No historical cause or flake-rate reduction
is claimed by this source proposal.
