# Reset writer barrier support, 26 October 2026

First owns the customer reset repair in #3579. Third closed duplicate #3583 without merging. Supporting code is on `third/reset-onboarding-state`, head `258b2a9f10`; it is not a complete reset repair and is not proposed for a competing merge.

`gate.json` records initial target-member barrier qualification: 186 passed, zero failed, one existing ignored fixture generator. `helper-gate.json` records 197 passed, zero failed, one existing ignored fixture generator after accepted Setup helpers, fixed Wi-Fi restore, journal fencing and exact credential sibling cleanup. Both retain actual runtime and compiler-source/executable bindings. No retry erased a failure.

Limits: the branch does not stop lingering lamp-setup.service or the independent skills-manage listener/instances. Those remain release HOLDs on First’s repair. No device was changed. The initial e5 head had native ARM64 40317153126 and Linux Clippy 40317153061 green; those results do not qualify the later helper commit.
