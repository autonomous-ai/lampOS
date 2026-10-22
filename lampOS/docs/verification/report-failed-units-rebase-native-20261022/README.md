# Failed-unit reports: rebased native qualification

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head `1299399e1dd6390361ff44cccf9145a9f8b9eb7f`.
Qualification complete for this PR CI scope: **yes**.

[Linux ARM64 run 39703359459](https://github.com/autonomous-ai/lampOS/actions/runs/39703359459) and
[Linux x86 Clippy run 39703359401](https://github.com/autonomous-ai/lampOS/actions/runs/39703359401)
both completed successfully. Run/API records, checkout logs and commit records
agree on merge `edccd3fe3b25382a9223f087c7758b4ae1587dd4`, with parents
`bf8c3f2e18695179bd70973821368f01d6a2d9e3` (tested base) and `1299399e1dd6390361ff44cccf9145a9f8b9eb7f` (PR head).

```text
Summary [ 171.446s] 9964 tests run: 9964 passed (1 leaky), 63 skipped
```

The primary nextest rows reconcile exactly: **9,963 PASS + 1 LEAK = 9,964**,
with no failures. The leaky result is
`lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
Later slow-test output repeats existing results and is not counted again.
All **3/3 added Cloud tests** passed; their names, times and log lines are in
[ci.json](ci.json).

Doc tests: **4 passed, 0 failed, 0 ignored**, across 46 targets.
The separate process harness: **12 passed, 0 failed**.
The separate relay harness emitted **one `relay_signals: ok`**.
Strict workspace/all-target x86 Clippy completed with **0 Rust/Cargo warnings**.

The whole workspace was selected, using PR profile `pr`, partition `hash:1/1`.
This is not full/nightly qualification: the realtime group is omitted, and
**63 tests were skipped**. The log does not list their individual names or
reasons. Nextest also excluded two custom-harness binaries, both run separately
as recorded above. API records show both cache-save steps and the ARM64
full-run-failure reporting job skipped; the nested platform-specific action
skip is recorded too.

ARM64 emitted **8 example-output collision warnings**
for `replay_cost`, `progress_cost` and their `.dwp` outputs. All warning headers
and continuation details are retained in ci.json, alongside certificate-rehash,
Node deprecation and runner annotations from both jobs. The original logs use
`UNKNOWN STEP` labels; command/result markers and API step conclusions are
kept distinct rather than assigning invented log step names.

Artifact metadata identifies `lampos-arm64-1299399e1dd6390361ff44cccf9145a9f8b9eb7f`,
ID **12393099606**, **112969340 bytes**.
Its API size/digest match the upload log. **The artifact payload was not
downloaded, hashed or inventory/checksum verified.** This is metadata evidence only.

The receipt binds all nine captured input files by SHA-256 and retains exact
result lines. [SHA256SUMS](SHA256SUMS) covers this page and ci.json. Previous-head
receipts remain separate. No physical lamp, live broker, device latency or
acoustic acceptance is claimed. Merge remains with the reviewer.
