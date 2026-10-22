# Requested-audio metadata diagnostics: native qualification

[PR #2871](https://github.com/autonomous-ai/lampOS/pull/2871), head `db7db7a5f5c029dd442aa2120f357b5af9149643`,
is **CI-qualified and ready for review**. The reviewer decides merge; this
receipt does not record a merge or deployment.

[ARM64 run 39716702065](https://github.com/autonomous-ai/lampOS/actions/runs/39716702065)
and [Linux Clippy run 39716702114](https://github.com/autonomous-ai/lampOS/actions/runs/39716702114)
both completed successfully. Their two run metadata records, checkout logs and
immutable commit parents agree on tested merge `4e0abb5416ee8005f010326275b548e7287604a9`:
base `4f2df6bf6bab82945cff4c998364427af80b6b27` plus the exact head above. This actual CI base
is later than the branch parent `2d1d353965b6b9f9bdc043ee4d19832630b3a6d8`.
Mutable embedded PR associations are not used as identity authority.

```text
Summary [ 172.794s] 10189 tests run: 10189 passed (1 leaky), 63 skipped
```

The 10,189 unique primary result rows reconcile with the summary and every
ordinal: 10,188 ordinary PASS rows and 1 LEAK row; no FAIL or TIMEOUT rows.
The leaky result is `lamp-voice` test
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
The whole workspace was selected under the PR profile; 63 skipped tests
remain skipped, and the log does not provide their complete names.

**All six added or strengthened diagnostic regressions passed**, each bound to
its exact `lamp-skills requested_audio::file_checks::` nextest row in
[ci.json](ci.json):

- Role/phase and nondisclosure: six combinations.
- Stamp-field and observation-side differences: twenty changes.
- Non-regular and symlink refusals: three replacements.
- Maximum-width numeric refusal: one ASCII line within 1,536 bytes.
- Real listed-path replacement with the opened descriptor unchanged.
- Parent replacement retaining its distinct folder refusal.

The two unchanged equal-stamp/content and SHA-256 encoding controls also passed.
The inspected source's Git blob matches the immutable tested merge; its
SHA-256 is `f4459837b0b129cf3f4645b07ba3bcb5d5310e7e9860adca11a42e22257bda56`. The original attempt/control
receipts remain separate historical evidence, not new native controls.

Doc tests: **4 passed / 0 failed / 0 ignored**.
Own-harness tests: **12 passed / 0 failed**.
Relay: **passed**. Strict Clippy ran
`cargo clippy --locked --workspace --all-targets -- -D warnings`
and emitted **0 Rust warnings**.
The ARM64 log retains 8 example-output collision warnings;
certificate and Node infrastructure notices are recorded separately.
All job and step terminal states are in `ci.json`; the signing/publication job
was skipped.

The artifact API lists one unexpired head-matching artifact. Its ID, size and
reported digest are retained as metadata; **the payload was not downloaded or
independently verified**. The separate [fresh Mac Skills gate](https://github.com/autonomous-ai/lampOS/blob/a62b9486cd47f0fe54949e712a18f19cb3775100/lampOS/docs/verification/requested-audio-metadata-rebase-2d1d3-20261022/README.md)
records 467 passed / 0 failed / 0 ignored / 0 filtered out on this head.

Full retained log SHA-256:

- ARM64: `71132bdb46c22cd2c4d0755e5bd224d0080a33c9f35551fd381464311915e0a3`
- Clippy: `a3e30dab98b0fa79af061462e20c47a57e448475229ed56f1daf1cdf4b10b96f`

[ci.json](ci.json) binds all nine raw inputs, source identity, selected exact
results, counts and limits. [SHA256SUMS](SHA256SUMS) covers this page and that
record. This qualification does not establish the original metadata-refusal
cause or physical-lamp, device, acoustic or latency behavior. Preparing this
proof ran no tests, builds, Git operations, network requests or device actions.
