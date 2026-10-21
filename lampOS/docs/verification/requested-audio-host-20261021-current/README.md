# Requested audio: current integration host verification, 21 October

The complete initial gate finished with **8,682 passed / 1 failed / 44 ignored /
0 filtered**. Its exit code remains 1 in [host.json](host.json). The failure is
an unchanged Setup loopback test; the later full Setup crate confirmation
passed **133 / 0 / 0 ignored / 0 filtered** with the exact same unit-test binary.
The intermittent failure remains unresolved. This record does not turn the
initial failed invocation into a passing one or claim native qualification.

Integration base: `99f2ecfcd4ebb3b9279750bee42f219c0c4895ed`.
The fresh source-only gate froze 2,180 non-document files with combined
SHA-256 `3313ed7eaec67ba7279ccf0f38ccbf2c44b9fb5493c9302c1df66439e53514d9`.
Every original and copied source hash still matched at completion. The
[consumer map](consumers.json) selects 36 crates from changed crates,
cross-crate name matches and reverse dependencies.

| Command group | Observed result |
|---|---|
| Workspace formatting | passed, 4.59 s |
| Strict workspace/all-target Clippy | passed, 138.82 s |
| 35 source-only consumer crates | 8,423 passed, 1 failed, 42 ignored, none filtered; 1,259.84 s |
| Full Update crate in checkout | 259 passed, 0 failed, 2 ignored, none filtered; 192.77 s |
| Workspace build | passed, 28.01 s |
| Full unchanged Setup confirmation | 133 passed, 0 failed, 0 ignored, none filtered; 28.77 s |

All focused checks passed separately: two file-proof tests, one scan-cost
test, 12 Voice integration tests, 49 Voice audio tests, 30 Skills tests and
one prepared-handoff test. The four renderer tests and render, package and
disposable installation passed. The 48-second WAV retained SHA-256
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`;
all seven installed files matched their staged hashes. Focused repeats and
the Setup confirmation are not added to the initial complete-suite totals.

This rebase preserves the newer optional caps controller and recovery
warnings alongside Music's speaker restriction and teardown guards. The
metadata scan implementation and its regression test file are unchanged
from the [previous complete gate](../requested-audio-host-20261021/README.md),
which records its own 8,620 passes and five mutation checks. Those full-gate
results qualify that earlier source tree.

The [current scan timing](scan-timing.json) measured 1.277500 milliseconds
first after load and 0.707583 / 0.702833 repeated, with zero payload bytes
read for sixteen 60-second recordings. The prepared host handoff measured
194 microseconds first and 180 repeated, from confirmation retirement to
authored speaker IPC acceptance. These measurements do not establish
cold-filesystem, native ARM64, acoustic or physical-lamp latency.

## Unchanged Setup failure

`keys::tests::the_check_is_one_request_with_the_key_in_its_header` accepted
an unexpected second TCP connection. Its assertion never reads that socket,
so the failure does not establish a second HTTP request or replayed key.
The Setup source and relevant dependencies match the earlier passing revision
and the integration base. [The diagnostic record](setup-intermittent.json)
retains source comparisons, every fixed diagnostic outcome, and raw-log hashes.

Nine isolated native runs of the exact binary produced eight passes and one
failure; a preceding sandbox launch was denied at the loopback bind. A private
instrumented copy produced ten passes, 90 complete first requests and no second
connection. The instrumented copy changed only the test helper and did not
change this PR or its frozen source. It did not identify the cause or prove a
fix. One subsequent full unchanged Setup invocation passed all 133 tests.
The failure is referred to Setup's owner; all original outcomes remain visible.
