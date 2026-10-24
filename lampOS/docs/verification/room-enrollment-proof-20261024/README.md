# Room attribution keeps this request’s enrollment proof

Third claimed reviewer-three’s merged #3306 proof-loss finding on the PR
and Queue before code. Own branch: `third/room-enrollment-proof`, base
`5074d860c`. The background known-file read replaced a same-request entry
with no enrollment proof. The extracted publisher reproduces that existing
behavior: **0/1**, valid proof becomes None (`publisher-before.txt`).

The corrected publisher carries the same proof and ScopeLease through
`saw_line_scoped`, already used by the forget path. It does not infer a proof
from room attribution. Focused enrollment controls **24/0**. The added case
uses an Engine-created microphone proof and verifies preservation, wrong
request, missing scope, switch-off and cancellation. It does not execute the
real background known-file I/O or camera/model loop.

First full Mac gate: formatting and strict all-target Voice Clippy passed.
Voice library **3,711 passed / 1 failed / 15 ignored**; the privacy kept-lease
resync test reported “privacy publisher unavailable” after ten attempts.
Integration targets and workspace build did not run after that failure.
Source hashes stayed unchanged. The failed test passed **3/3** exact isolated
serial observations, retained individually. Those passes do not erase the
full failure or establish its cause. One full serial rerun is now running on
unchanged source, followed by workspace build only if successful.

No Mac ARM64 link or device action. The combined-switches physical row is
on the enrollment trial sheet, routed to the holder/First for the 15:00 hands
batch. Switch remains off. Immediate publication adds no I/O/inference; its
under-1-ms target and actual acoustic/device latency remain unverified.
