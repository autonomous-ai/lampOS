# PR #3190 exact-head terminal CI

Both required workflows and their actual check jobs succeeded for authored head
`d5d0383accafac8aa54c352eba65b1b0d5a44d9d` on parent `cf462adb04f64d699c226dd3b1781c0ad927068f`:

- [ARM run 39934531704](https://github.com/autonomous-ai/lampOS/actions/runs/39934531704),
  job `120350713375`, completed 2026-10-23T10:00:40Z.
- [Clippy run 39934531599](https://github.com/autonomous-ai/lampOS/actions/runs/39934531599),
  job `120350713300`, completed 2026-10-23T09:51:02Z.

Both checked synthetic merge `7f7a04c2618367ac56227d3f774f08dae63b0f95`, whose parents are the
base above and the authored head. Its tree `278baa2eace61124fe255386404dc84410b446a8` equals the
authored head tree. All 8 authored complete files and four captured
CI configuration/script blobs match. [Source binding](identity/source-binding.json)
records these exact comparisons. This receipt qualifies only the recorded
checked source; a later carry or latest-parent decision remains separate.

ARM nextest: **7,215 passed, including 1 leaky pass; 0 failed;
32 tests and 106 binaries skipped**, in **144.082 s**.
It selected **12 of 46 packages** through PICKED,
with release builds, profile `pr`, default filter `all()`, `-j num-cpus` and
partition `hash:1/1`. This is not every workspace runtime test. All ordinals
1–7215 are present; 34 identical reprinted rows count once.
Startup names 62 binaries; terminal rows identify 58.
Skipped identities and reasons are not guessed. [Summary](summary.json),
[binary census](arm64/binary-census.json) and [actual commands/steps](arm64/receipt.json)
keep scope and toolchain observations explicit.

All 86 clock cases and the separate loading-child case passed. [Selected cases](arm64/selected-cases.json) preserves exact rows and
any uniquely mapped Actions-masked name. Total case time is not response latency.
The disclosed leaky pass and every failure row are retained in the summary.

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
