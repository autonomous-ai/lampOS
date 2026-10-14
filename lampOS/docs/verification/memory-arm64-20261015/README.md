# Memory native ARM64 verification

Ready for lamp-voice: yes (#743)

These are GitHub-hosted native Linux ARM64 runner results, not lamp runs.
The workflow `.github/workflows/lampos-arm64.yml` uses Ubuntu 22.04 ARM64
and Rust 1.96.0 as an ordinary user. Its release binaries are intended to
run on the lamps' Debian 12; this does not establish device behavior.

## Previous checkpoint: run 38610164757

Source `f411ad6008c9cabfb55b55caa7176045deeb3bcf`, based on
`dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`, completed successfully.
Release workspace binaries/examples built; the serial release test gate
passed **3,988 tests / 0 failed / 32 ignored, 269 completed suites**,
including the custom turn-end-wire process harness's 12 passes. Hashing
and artifact upload also succeeded. The retained run JSON records the
head, workflow steps and terminal conclusion; the raw `.txt` is unmodified
and the count JSON lists every completed suite and its raw SHA-256.

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

The full raw-log SHA-256 is
`f3030488ded47ce977bd816536164d3b9c993f3a55cce8215bc6964c697d5414`.
The workflow reported these previous-checkpoint binaries:

| Binary | SHA-256 |
|---|---|
| `lamp-memory` | `23521b5cf412ee530b6953f8edd406b3b8bf250a535d3a1cdbfed081180efca9` |
| `lamp-voice` | `d072fe8ebe268bce8a2a4c46877f507f6c72ae8451ff282a99eca6266fc5b93b` |
| `lamp-tasks` | `d739ff6b1526ab5565feb74a17bd293b57199b0ec4bd2e2ee72794e3afce95a6` |

This source's complete Mac gate remains failed, as preserved in
`../mac-workspace-20261015/`; its runtime/build files match the Mac source
`5fa0f78d6`, with only two verification documents changed. The native result
is separate evidence and does not establish the Mac failures' cause.

## Intermediate checkpoint: run 38611490112

Source `56b458966ffd10a7938d551de0449cf9fbc723bd`, based on First's
Tasks repair `1a38f2f3e6b8afd1e7896761a0da6149a2400f67`, also completed
successfully at 18:49:32 UTC on 2026-10-14: **3,995 passed / 0 failed /
32 ignored, 269 suites**, including the 12-test custom process harness.
Release workspace binaries/examples built and the hashes and artifact
upload succeeded. Its full raw SHA-256 is
`444a7205ef1b7c293d010bd8914d7e6fabf1478b083854c718530bcd76c5c022`.

| Binary | SHA-256 |
|---|---|
| `lamp-memory` | `23521b5cf412ee530b6953f8edd406b3b8bf250a535d3a1cdbfed081180efca9` |
| `lamp-voice` | `f048229add5cf165c385ece63e96cf2d659ba0ede572f1b3a552ca586d2c4801` |
| `lamp-tasks` | `946a585d18f0c9ce29eb6ed975eb37e030af10c80aac11f1c714ca8c72fa7a8f` |

The exact run JSON and full raw/count files are adjacent. Its artifact is
[11919369471](https://github.com/autonomous-ai/lampOS/actions/runs/38611490112/artifacts/11919369471).
The same source's complete ordinary-user native x86_64 Linux gate also
passed 3,995 / 0 / 32 across 269 suites, with formatting, strict workspace
Clippy, workspace build and the three ARM64 cross-builds; those local
results remain in `../memory-linux-20261015/`.

## Qualified task head: run 38615610362

Source `375f6f7625734ed7db80c62b020b6e8e32df7d82`, based on
`d8e9556dea3f1867a5af5297d1ebddbd165d9102`, also carries the newly merged
upstream task-news, Body and Touch changes. All nine Memory task
source/test/manifest files remain unchanged. Native ARM64 run 38615610362
ran from 18:49:39 to 19:17:02 UTC on 2026-10-14 and succeeded for this exact
head: **4,245 passed / 0 failed / 32 ignored, 280 completed suites**.
That includes 4,233 standard libtest/doctest passes and the custom process
harness's 12 passes. Release workspace binaries/examples built, and every
workflow step, including hashing and artifact upload, succeeded. The
adjacent unmodified log has SHA-256
`713e9dd4f3e7ca54065ee9e7d47f47db56040e2acd8960bc02e5643dbb842a90`.

| Binary | SHA-256 |
|---|---|
| `lamp-memory` | `47193be138debd6c526eb6ccd4d5cde94de09d3068aaad1317b14441bf91390a` |
| `lamp-voice` | `edb05d70f5731df664f7dfe2d5e27c6ff437b1c152f24d31db391dd9025c5395` |
| `lamp-tasks` | `94613935ff3e4949262547db1e4f3be8a1ccb8a7d02d2233b8b5bd792df6705a` |

The binaries are in [artifact 11921159730](https://github.com/autonomous-ai/lampOS/actions/runs/38615610362/artifacts/11921159730).
Memory is offered as non-draft [PR #743](https://github.com/autonomous-ai/lampOS/pull/743).
The local final-head strict lint/build checks are separate from this full
native ARM64 test run. Upstream advanced to `1e5beebf4` during the run;
its additional Body/Tasks/task-news changes do not touch this task's source
files. These results qualify the stated task head, not a later integration
commit. First owns the lamp check and merge.

## Not shown

No deployment, lamp execution, flash durability, physical privacy or
end-to-end acoustic latency. ARM64 execution here was on a CI runner.
