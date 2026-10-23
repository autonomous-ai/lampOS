# PR #3190 historical carry terminal CI

Both required workflows and their actual check jobs succeeded for authored head
`0c2cb3df2672d08f580f0c6ccad9781f054bdbf9` on parent `6b52c916a3cfcc26a2c97670b1a9ebab4c84d174`:

- [ARM run 39908570990](https://github.com/autonomous-ai/lampOS/actions/runs/39908570990),
  job `120266045049`, completed 2026-10-23T06:09:00Z.
- [Clippy run 39908570985](https://github.com/autonomous-ai/lampOS/actions/runs/39908570985),
  job `120266045368`, completed 2026-10-23T06:05:15Z.

Both checked synthetic merge `d800e03641b5df1239a75ff789e1a51b9365676c`, whose parents are the
base above and the authored head. Its tree `fbf70a0a34e617bd71aae6a6f7e8775929dd29af` equals the
authored head tree. All 8 authored complete files and four captured
CI configuration/script blobs match. [Source binding](identity/source-binding.json)
records these exact comparisons. This result is historical to the later
cf462 carry and does not qualify that newer source.

ARM nextest: **7,173 passed, including 1 leaky pass; 0 failed;
32 tests and 106 binaries skipped**, in **141.890 s**.
It selected **12 of 46 packages** through PICKED,
with release builds, profile `pr`, default filter `all()`, `-j num-cpus` and
partition `hash:1/1`. This is not every workspace runtime test. All ordinals
1–7173 are present; 34 identical reprinted rows count once.
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

The [own-PR read](comments.json) found no new reviewer request since the prior
read. Earlier execution HOLDs and local failures remain unwaived. These jobs did
not run Linux formatting. The obsolete 0c2 formatting attempt timed out while
creating its source archive, before any formatter ran; it is not a formatting
pass. New exact-head plans are separate and unexecuted at this collection.

Full [ARM](arm64/job.log.json) and [Clippy](clippy/job.log.json) logs are lossless
UTF-8 JSON, with original byte counts and SHA256. Portable API projections omit
person attribution; [metadata records](metadata-projections.json) bind original
external responses. The [collection record](collection.json) preserves the job
selection/parser corrections as collection errors, not CI failures or reruns.
[External records](external-files.json), [validation](validation.json) and
[SHA256SUMS](SHA256SUMS) complete the package. No local build/test, workflow retry,
source change, device action or external post was performed by this collection.
