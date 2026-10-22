# Failed-unit reports: rebased native qualification

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head `a50f88224132066826946c452b1a52fd44568459`.
Qualification complete for this exact head's PR CI scope: **yes**.
This receipt applies only to this exact head and tested merge; it does not
qualify a later rebase or any other head.

[Linux ARM64 run 39711676351](https://github.com/autonomous-ai/lampOS/actions/runs/39711676351) and
[Linux x86 Clippy run 39711676412](https://github.com/autonomous-ai/lampOS/actions/runs/39711676412)
both completed successfully. Run/API records, checkout logs and commit records
agree on merge `42dc45153811c933b55c7124fe0d54ce894f6544`, with parents
`17fdfda1d757324b8437d997304e855e8ecbf9dd` (tested base) and `a50f88224132066826946c452b1a52fd44568459` (PR head).
The API's embedded `pull_requests` association is mutable and may reflect a
later rebase. Its observed head/base are retained separately in ci.json and are
not treated as tested identities. Qualification requires both run head hashes,
the checkout merge and immutable parents to agree.

```text
Summary [ 171.943s] 10007 tests run: 10007 passed (1 leaky), 63 skipped
```

The primary nextest rows reconcile exactly: **10,006 PASS + 1 LEAK = 10,007**,
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
as recorded above. Skipped API steps/jobs: arm64: record fresh source times for a cold publication build; arm64: discard the isolated example target before saving the cache; arm64: save the cache; clippy: save the cache; arm64: lampOS — sign and publish the tested lamp-voice artifact; arm64: report-full-run-failure.
Nested action skips, when present, are recorded separately in ci.json.
The publication job was skipped. This offline receipt does not inspect or
certify repository variables, environment settings or secret provisioning.

ARM64 emitted **8 example-output collision warnings**.
Actual warning categories, counts, headers and continuation details from both
jobs are retained in ci.json. Original log labels,
including any `UNKNOWN STEP` labels, are counted in ci.json. Command/result
markers and API step conclusions remain distinct; no log step names are invented.

Artifact metadata identifies `lampos-arm64-a50f88224132066826946c452b1a52fd44568459`,
ID **12396734858**, **113235555 bytes**.
Its API size/digest match the upload log. **The artifact payload was not
downloaded, hashed or inventory/checksum verified.** This is metadata evidence only.

The receipt binds all nine captured input files by SHA-256 and retains exact
result lines. [SHA256SUMS](SHA256SUMS) covers this page and ci.json. Previous-head
receipts remain separate. No physical lamp, live broker, device latency or
acoustic acceptance is claimed. Merge remains with the reviewer.
