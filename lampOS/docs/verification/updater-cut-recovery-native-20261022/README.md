# Updater cut recovery: native evidence

[PR #2885](https://github.com/autonomous-ai/lampOS/pull/2885), head `b0e8bf3b9f1cbbc054c5962961f656ea6109d1bf`:
**positive native CI passed; #2810 acceptance is not asserted here**.
[ARM64 run 39719994995](https://github.com/autonomous-ai/lampOS/actions/runs/39719994995) and
[Linux Clippy run 39719994996](https://github.com/autonomous-ai/lampOS/actions/runs/39719994996) both completed successfully.
Both checkout logs and captured commit records identify merge `5942f2a7e1d464eb8af17e20046cc7897f3121c7`,
with parents `3095487ad6330c89566e7d6db6208dcc882f47e9` and the exact PR head above.
Embedded live PR associations are recorded separately from immutable identity evidence.

```text
Summary [ 173.417s] 10193 tests run: 10193 passed (1 leaky), 63 skipped
```

All 10,193 primary identities and ordinals reconcile: **10,192 PASS + 1 LEAK**,
no failures or timeouts. The 35 result rows replayed after the summary are not
counted again. The leaky pass is `lamp-voice`
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
The PR profile selected the whole workspace: 162 binaries ran, 63 tests and
2 own-harness binaries were skipped by nextest. The complete names of the
63 skipped tests are not emitted in the log.

All **six `lamp-update::round self_cuts::` fixtures passed** at primary log
lines 6705–6709 and 6711. [ci.json](ci.json) records each exact name, ordinal,
result and duration, and binds the six declarations to source blob
`218b47eb12dcec77be9e81ecfd8023b7e73ca199` in the tested merge.
The source SHA-256 is `9ce086ae4ca867c1c7383c59aac34d91feee45a652e88a066659b6b37513550c`.

Separate doc tests: **4 passed / 0 failed / 0 ignored**, across 46 crate summaries.
The custom process harness: **12 passed / 0 failed**. Relay: **passed**.
Strict workspace/all-target Clippy completed with **0 Rust warnings**.
The 8 ARM64 Cargo example-output collision warnings, plus certificate rehash
and Node deprecation notices in both runs, are recorded with log lines in JSON.
Every job and step terminal state is retained. Signing/publication was skipped;
so were the PR-ineligible cache-save and example-cache cleanup steps.

The artifact API reports one unexpired head-matching artifact, ID
`12400353878`, **113,670,522 bytes**. Its reported digest and the
CI export statement are retained as metadata. **No artifact payload was
downloaded or independently verified for this receipt.**

The [separate host gate and controls](https://github.com/autonomous-ai/lampOS/blob/b0e8bf3b9f1cbbc054c5962961f656ea6109d1bf/lampOS/docs/verification/updater-cut-recovery-20261022/README.md)
remain distinct from these positive native runs. R1–R5 break the selected
`current/lamp-update` entrypoint while retained old/new copies or the fixed
independent fallback remain. They do not show every executable removed or
both versioned copies unable to run. S1–S6 cover state/order and O1 covers
opt-in. **First owns whether these controls meet #2810's acceptance wording.**

The post-refusal/pre-cleanup subcase reconstructs pending from captured bytes
after real put-back; it is **not an actual kill or execution cut after refusal**.
The fixtures test logical persisted prefixes and executable shell stubs,
not physical power loss, fsync durability, real timer firing or daemon exec.
These CI runs do not establish device behavior or product latency.

Full retained log SHA-256:

- ARM64: `1efcae224df70c9ea580e4c646352ec24e955c2a12f45ce22048836bc6088b71`
- Clippy: `5148e22cab6f4663414359d90a287d2de469fc1f693ec2e36aff9aa22482dacb`

[ci.json](ci.json) binds all nine raw inputs, exact identities, counts,
warnings, skipped steps and limits. [SHA256SUMS](SHA256SUMS) covers this page
and that record. The original logs remain unchanged; their `UNKNOWN STEP`
labels were not replaced or inferred from timestamps. Preparing this receipt
ran no tests, builds, Git commands, network requests or device actions.
