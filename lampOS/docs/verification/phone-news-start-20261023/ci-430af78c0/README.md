# PR #3218 historical carry terminal CI

Both required workflows and their actual check jobs succeeded for authored head
`430af78c0a1a785c2f422fc62d7bbdcc80103d4d` on parent `6b52c916a3cfcc26a2c97670b1a9ebab4c84d174`:

- [ARM run 39908229542](https://github.com/autonomous-ai/lampOS/actions/runs/39908229542),
  job `120264988637`, completed 2026-10-23T06:06:38Z.
- [Clippy run 39908229318](https://github.com/autonomous-ai/lampOS/actions/runs/39908229318),
  job `120264988973`, completed 2026-10-23T05:55:28Z.

Both checked synthetic merge `4b724f7447003e84b2648c403b392552e6c38d64`, whose parents are the
base above and the authored head. Its tree `f6d053a93e642389a1a5d5bcca21180c4bd6e5e0` equals the
authored head tree. All 2 authored complete files and four captured
CI configuration/script blobs match. [Source binding](identity/source-binding.json)
records these exact comparisons. This result is historical to the later
cf462 carry and does not qualify that newer source.

ARM nextest: **7,173 passed, including 1 leaky pass; 0 failed;
32 tests and 106 binaries skipped**, in **142.002 s**.
It selected **12 of 46 packages** through PICKED,
with release builds, profile `pr`, default filter `all()`, `-j num-cpus` and
partition `hash:1/1`. This is not every workspace runtime test. All ordinals
1–7173 are present; 34 identical reprinted rows count once.
Startup names 62 binaries; terminal rows identify 58.
Skipped identities and reasons are not guessed. [Summary](summary.json),
[binary census](arm64/binary-census.json) and [actual commands/steps](arm64/receipt.json)
keep scope and toolchain observations explicit.

The changed phone-news case passed. [Selected cases](arm64/selected-cases.json) preserves exact rows and
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
