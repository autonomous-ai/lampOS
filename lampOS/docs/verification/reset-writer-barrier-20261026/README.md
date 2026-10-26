# Reset writer barrier support, 26 October 2026

First owns the customer reset repair in #3579. Third closed duplicate #3583 without merging. Supporting code is on `third/reset-onboarding-state`, head `258b2a9f10`; it is not a complete reset repair and is not proposed for a competing merge.

`gate.json` records initial target-member barrier qualification: 186 passed, zero failed, one existing ignored fixture generator. `helper-gate.json` records 197 passed, zero failed, one existing ignored fixture generator after accepted Setup helpers, fixed Wi-Fi restore, journal fencing and exact credential sibling cleanup. Both retain actual runtime and compiler-source/executable bindings. No retry erased a failure.

Limits: the branch does not stop lingering lamp-setup.service or the independent skills-manage listener/instances. Those remain release HOLDs on First’s repair. No device was changed. The initial e5 head had native ARM64 40317153126 and Linux Clippy 40317153061 green; those results do not qualify the later helper commit.

## Authoritative First PR review

`first-cbc-host.json` qualifies First’s exact `cbc89589b` head independently: clean 48 local packages, fmt, strict Setup Clippy, workspace build and full Setup 185/0/1. All five actual and preserved executables and source hashes stayed unchanged. `first-cbc-source-read.json` records the closed findings and remaining limits. Third posted READ-OK on PR3579; installed reset/customer onboarding remains First’s acceptance.
