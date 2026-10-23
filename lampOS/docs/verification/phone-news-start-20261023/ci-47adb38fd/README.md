# PR #3218 terminal CI receipt

Both required workflows and their actual check jobs succeeded on authored head
`47adb38fd0f61dfbfc1172e9a668d54c43983794`:

- [ARM run 39903799979](https://github.com/autonomous-ai/lampOS/actions/runs/39903799979),
  job `120251037039`, completed **05:25:36Z** on 23 October 2026;
  workflow completed at **05:25:37Z**. Job duration **20m 22s**.
- [Clippy run 39903799949](https://github.com/autonomous-ai/lampOS/actions/runs/39903799949),
  job `120251036901`, completed **05:08:06Z**. It ran
  `cargo clippy --locked --workspace --all-targets -- -D warnings` on
  Rust 1.96.0 x86 Linux. No runtime tests are claimed from Clippy.

Both logs show checkout `0faa46c6d1bc8d3d8e7545afb2a90a74c8374abf`,
whose parents are base `62b1120fc1af5462529f0c76089a322500e30929` and
own head `47adb38fd0f61dfbfc1172e9a668d54c43983794`; its tree is
`6574a2cba5f6050670451818f5350222ed74b9be`. This is a newer whole tree than
the Mac gate on e5. Both authored phone blobs and all four captured CI
configuration files equal the authored head. [Identity](identity/source-binding.json)
and [configuration](identity/ci-configuration.json) retain the exact hashes.

ARM nextest reports **7,171 passed, including one leaky pass; 0 failed;
32 tests and 106 binaries skipped**, in **142.620 s**. Compilation for that
step took a separately reported **11m 46s**. It used Rust 1.96.0 aarch64 Linux,
nextest 0.9.148, `--release --workspace --profile pr -j num-cpus`, four observed
cores and partition `hash:1/1`. The actual PICKED filter selected **12 of 46
packages**. The profile default filter is `all()`; the realtime group is
included. Runtime coverage is selected-package coverage, not every workspace
test. [Summary](summary.json) records the full package list and command scope.

The changed phone case **passed in 3.066 s**, ordinal 4033. This is total case
duration including its unchanged quiet window, not response latency.
The leaky pass is
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`,
**0.223 s**, ordinal 5052. [Selected cases](arm64/selected-cases.json) retains
both exact rows. All six disclosed Mac failure names passed here; different
platform/profile/checked tree make these later observations, not causal
controls or waivers of those retained failures.

All ordinals 1–7171 are present. Thirty-four identical final-output repetitions
are counted once. Startup lists 62 binaries; terminal rows identify 58. The log
does not name skipped tests/binaries or the four starting binaries with no
terminal row, so their identities/reasons are not guessed.
[Binary census](arm64/binary-census.json) binds these counts to the complete log.

Separate commands passed **4 doctests across 46 doc targets**, **12 process
harness cases**, and `relay_signals: ok` (no numeric case count emitted).
Offline publication suites separately passed **35**, **12** and **7** tests.
These are not added to nextest totals. [Harness records](arm64/separate-harnesses.json)
retain boundaries, actual names and summaries. Release build, example builds,
classification and artifact export/upload succeeded. The artifact uploader
reported 47 binaries, 2 selected examples and 50 files; its ZIP digest/size are
recorded, but the artifact was not independently downloaded or hashed. Cargo
example filename-collision warnings during nextest compilation remain visible
in the full raw log and summary. Two downstream jobs (failure reporting and
sign/publication) were skipped; neither is counted as a passed check.

The [final PR/comment read](comments.json) found one Tests status comment, no
reviews, no inline review comments and no review threads. No addressed reviewer
finding was observed. The PR remained draft. The native successes do not clear
the six Mac failures or authorize readiness/merge. These workflows ran no
Linux formatting check; root's formatting receipts remain separate.

Full [ARM log](arm64/job.log.json) and [Clippy log](clippy/job.log.json) are
lossless UTF-8 JSON strings with exact original byte counts and SHA256 values.
No log redaction was needed. Portable API records omit person attribution;
[projection metadata](metadata-projections.json) binds the complete external
responses, and [attribution scan](attribution-scan.json) states its scope.
The original watcher connection failure and the collector's corrected one-job
assumption are retained externally as collection observations, not CI failures.
No workflow rerun, local build, test, device action, branch write or post occurred.
[External receipts](external-files.json), [validation](validation.json) and
[SHA256SUMS](SHA256SUMS) complete this package.
