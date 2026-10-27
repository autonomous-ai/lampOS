# Restart 1a8: original native CI

Both original attempt-1 pull-request runs for [#3572](https://github.com/autonomous-ai/lampOS/pull/3572), offered head `1a8a859a1658f2a730143be82dd3a8be972d7838`, succeeded:

- [ARM64 40438366476](https://github.com/autonomous-ai/lampOS/actions/runs/40438366476): **7,555 unique PASS; 0 FAIL, LEAK, TIMEOUT or unrun selected cases**, 53 aggregate skipped tests. The actual selection is twelve packages, profile `pr`, partition `hash:1/1`; 70 binaries are reported selected, 62 have nonempty PASS records and 115 binaries are skipped.
- [Linux 40438366553](https://github.com/autonomous-ai/lampOS/actions/runs/40438366553): workspace formatting and strict all-targets Clippy, plus separate promoter formatting/Clippy, passed.

Both job logs check out actual preview `90bc0585d10918d5bbd6a2a5836f114028fec2f9`, ordered parents **`33d5627cadad7184f9ea2dcdc5b858f3aa3de58d`, then `1a8a859a1658f2a730143be82dd3a8be972d7838`**, tree `4762d95af11b4d7dfd6e0c39d629e4f4a45acc5d`. This 10,677-entry tree differs from the offered head's tree at seven upstream Body/Voice/docs paths. Both authored files remain byte-identical by Git blob/mode binding. The offered head's own parent is `737331535d0cab813170d288110fc44ff5d41a68`. No source or binary equivalence to another gate is inferred.

All **21 `same_voice::tests::` cases PASS once**, including the changed hung-child case and the journal-worker case. All fourteen ordinary Linux Reader cases also PASS once. The privileged isolated-root/setpriv DAC test remains ignored/unrun, and the non-Linux fallback is not compiled. No physical model, lamp hardware or cross-identity DAC qualification is established.

Execution records occupy raw ARM log lines **833–8387**, between the Starting line 832 and Summary line 8389. There are no duplicate execution identities or missing result ordinals and no nextest retry/flaky records. **34 later PASS records** match earlier identities, durations and ordinals: 22 realtime and 12 privacy-worker records, reprinted by actual `success-output = "final"` rules before the docs step at line 8806. They add no tests. Fixture-internal retry loops are outside that accounting claim.

Separate successful results are **promoter 25/0/0, workspace docs 4/0/0, process harness 13/0 and relay harness `ok`**. These are not added to nextest totals. Publication regression/rehearsal and export/upload steps pass; actual promotion/signing jobs are skipped for this PR.

Artifact **12705432204**, 116,140,938 bytes, has API digest `sha256:19ea96715ef8f55dbe39d5bf906aa4409583e5d28bb7de052879628d673ed383`, identical to the upload-log zip digest. Export reports 47 current workspace binaries, two selected examples and complete `SHA256SUMS`. The archive and individual payload bytes were not downloaded or independently verified; no ARM binary was run locally.

The [historical 11,356 PASS plus one LEAK](../tests-restart-leak-20261026/) remains unchanged and causally unproved. The [earlier a209 host](../tests-restart-a209-host-20261027/) and [a209 native preview](../tests-restart-a209-native-20261027/) retain their own sources. This current original native result applies to the explicit 33d/1a8 preview; later-base readiness and the unchanged never-ready fixture follow-up remain separate.

The combined PR comment/review timeline has no new item after comment 6432324760's timestamp (2026-10-27 02:34:42 UTC). [audit.json](audit.json) binds source, full case records and private raw receipt digests. A separate post-writer-close verification checked every collection-manifest entry with zero exclusions. No CI rerun/write, source/target/compiler action or artifact execution was performed.
