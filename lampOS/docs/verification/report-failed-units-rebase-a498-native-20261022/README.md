# Failed-unit reports: historical rebased native qualification

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head `1114e4b8f2030cac4ca1315d94bd07f94cf6ecd9`.
Qualification complete for this exact head's PR CI scope: **yes**.
**Historical checkpoint:** this receipt does not qualify the later candidate
`87154f053` or any subsequent head.

[Linux ARM64 run 39706852548](https://github.com/autonomous-ai/lampOS/actions/runs/39706852548) and
[Linux x86 Clippy run 39706852627](https://github.com/autonomous-ai/lampOS/actions/runs/39706852627)
both completed successfully. Run/API records, checkout logs and commit records
agree on merge `8a74f8d02294b347e0a1680ef880e428cbc90f5e`, with parents
`a498ecc030a9c28419aef3b7de89486664868440` (tested base) and `1114e4b8f2030cac4ca1315d94bd07f94cf6ecd9` (PR head).

```text
Summary [ 185.137s] 9998 tests run: 9998 passed (1 leaky), 63 skipped
```

The primary nextest rows reconcile exactly: **9,997 PASS + 1 LEAK = 9,998**,
with no failures. Leaky results: `lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
Output after the nextest summary is excluded from these primary counts.
All **3/3 added Cloud tests** passed; their names, times and log lines are in
[ci.json](ci.json).

Doc tests: **4 passed, 0 failed, 0 ignored**, across 46 targets
(0 measured, 0 filtered).
The separate process harness: **12 passed, 0 failed**.
The separate relay harness emitted **one `relay_signals: ok`**.
Strict workspace/all-target x86 Clippy completed with **0 Rust/Cargo warnings**.

The whole workspace was selected, using PR profile `pr`, partition `hash:1/1`.
This is not full/nightly qualification: the realtime group is omitted, and
**63 tests were skipped**. No individual SKIP rows are logged. Individual names/reasons cannot be reconstructed from these inputs.
Nextest also excluded two custom-harness binaries, both run separately
as recorded above. Skipped API steps/jobs: arm64: record fresh source times for a cold publication build; arm64: discard the isolated example target before saving the cache; arm64: save the cache; clippy: save the cache; arm64: report-full-run-failure; arm64: lampOS — sign and publish the tested lamp-voice artifact.
Nested action skips, when present, are recorded separately in ci.json.

ARM64 emitted **8 example-output collision warnings**.
Actual warning categories, counts, headers and continuation details from both
jobs are retained in ci.json. Original log labels,
including any `UNKNOWN STEP` labels, are counted in ci.json. Command/result
markers and API step conclusions remain distinct; no log step names are invented.

Artifact metadata identifies `lampos-arm64-1114e4b8f2030cac4ca1315d94bd07f94cf6ecd9`,
ID **12396016196**, **113238534 bytes**.
Its API size/digest match the upload log. **The artifact payload was not
downloaded, hashed or inventory/checksum verified.** This is metadata evidence only.

The receipt binds all nine captured input files by SHA-256 and retains exact
result lines. [SHA256SUMS](SHA256SUMS) covers this page and ci.json. Previous-head
receipts remain separate. No physical lamp, live broker, device latency or
acoustic acceptance is claimed. Merge remains with the reviewer.
