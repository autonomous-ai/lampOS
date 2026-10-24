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

## Completed local gate and pushed source

Authored head `f653bc5bdcdaecb661f27aa076f395dea38474c7`. The unchanged source passed the full serial Voice
command: **3,993 passed / 0 failed / 21 ignored**, including library
**3,712/0/15** and all **270** integration cases. Six extra ignored cases
are manual release-pair controls, not executed here. Workspace build passed
afterward. Source hashes match across formatting/strict Clippy, both whole
test attempts and build, and every bound file matches its authored git blob.

`final-gates.json`, `voice-serial.txt` and `workspace-build.txt` retain the
terminal checks. The initial failed library and all isolated reruns stay
above. Official ARM64/Linux Clippy, two public privacy reads and Linux fmt
remain distinct pending gates.

## Official native result and first privacy read

#3344 `f653bc5bd` passed ARM64 run 40072776658: selected suite
**7,351 passed (one reported Voice journal LEAK), 45 skipped**, process
**12/0**. Linux Clippy 40072776682 passed. Preview `c3566b8e7` carries
base `411965dcb`; all four authored files match, while six unrelated base
files differ. This is not whole-tree equality. `native.json` binds the raw
log hash, preview parents, exact differences and result.

The lead’s READ-OK is in with two nonblocking notes: with enrollment on
and no lease, the whole room update is dropped; and a direct epoch-bump
interleaving is untested by the new helper regression. Both are explicit
in the PR body. Existing shared proof/forget/privacy checks and the unrun
physical sheet remain distinct. Second privacy read and exact Linux fmt
are pending; author HOLD is only for the two-reader privacy requirement.
