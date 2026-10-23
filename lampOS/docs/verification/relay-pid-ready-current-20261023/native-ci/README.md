# PR #3255 exact-head terminal CI

Both required workflows and their actual check jobs succeeded for authored head
`e228f645212fa32d6e9c1446f6d01621fe2c563d` on parent `cf462adb04f64d699c226dd3b1781c0ad927068f`:

- [ARM run 39934214655](https://github.com/autonomous-ai/lampOS/actions/runs/39934214655),
  job `120349648555`, completed 2026-10-23T09:57:00Z.
- [Clippy run 39934214581](https://github.com/autonomous-ai/lampOS/actions/runs/39934214581),
  job `120349648622`, completed 2026-10-23T09:48:46Z.

Both checked synthetic merge `42fabceba55b7bf579d3e13ed286d28b469f8707`, whose parents are the
base above and the authored head. Its tree `450e6d02f95b422e15f3c1a2173a0c0e34b52d9f` equals the
authored head tree. All 2 authored complete files and four captured
CI configuration/script blobs match. [Source binding](identity/source-binding.json)
records these exact comparisons. This receipt qualifies only the recorded
checked source; a later carry or latest-parent decision remains separate.

ARM nextest: **1,162 passed, including 0 leaky pass; 0 failed;
4 tests and 154 binaries skipped**, in **26.164 s**.
It selected **3 of 46 packages** through PICKED,
with release builds, profile `pr`, default filter `all()`, `-j num-cpus` and
partition `hash:1/1`. This is not every workspace runtime test. All ordinals
1–1162 are present; 0 identical reprinted rows count once.
Startup names 14 binaries; terminal rows identify 12.
Skipped identities and reasons are not guessed. [Summary](summary.json),
[binary census](arm64/binary-census.json) and [actual commands/steps](arm64/receipt.json)
keep scope and toolchain observations explicit.

The changed relay PID-ready case passed. [Selected cases](arm64/selected-cases.json) preserves exact rows and
any uniquely mapped Actions-masked name. Total case time is not response latency.
No leaky pass or failure row was observed.

Separate commands passed **4 doctests across 46 doc targets**,
**12 process cases**, and the relay harness (`ok`, no numeric count emitted).
Offline publication suites passed **35**, **12** and **7** tests separately.
[Harness evidence](arm64/separate-harnesses.json) retains all target headers and
summaries, including the stdout/stderr interleaving that needs sequence-based
rather than immediate-adjacency parsing. These counts are not added to nextest.
Artifact upload digests are raw-log observations; no artifact ZIP was downloaded.
Restored caches/source times mean this is not a cold-build measurement.

The [own-PR read](comments.json) records the latest review/status comments.
Receipt integrity and readiness disposition remain separate; no failure or
review HOLD is waived here. These jobs did not run Linux formatting. A separate
source-only Linux x86 check passed for this exact authored head; its
[command/toolchain/source receipt](https://github.com/autonomous-ai/lampOS/tree/c548067c1a27aeb59816902df48e4674c164e6ba/lampOS/docs/verification/test-carries-linux-format-20261023)
is published separately. The earlier 0c2
archive timeout remains a failed preparation, with no formatter invocation.

Full [ARM](arm64/job.log.json) and [Clippy](clippy/job.log.json) logs are lossless
UTF-8 JSON, with original byte counts and SHA256. Portable API projections omit
person attribution; [metadata records](metadata-projections.json) bind original
external responses. The [collection record](collection.json) preserves the job
selection/parser corrections as collection errors, not CI failures or reruns.
[External records](external-files.json), [validation](validation.json) and
[SHA256SUMS](SHA256SUMS) complete the package. No local build/test, workflow retry,
source change, device action or external post was performed by this collection.
