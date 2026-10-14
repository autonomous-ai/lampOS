# Memory native ARM64 verification

Ready for lamp-voice: no

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

## Intermediate checkpoint: pending

First requested the later Tasks repair in `1a38f2f3e`, and the task was
rebased to `56b458966ffd10a7938d551de0449cf9fbc723bd`. Run 38611490112
is now building that exact head. The success above does not qualify this
newer source. Its complete ordinary-user native x86_64 Linux gate finished
with 3,995 passed / 0 failed / 32 ignored across 269 suites; formatting,
strict workspace Clippy, workspace build and the Memory/Voice/Tasks ARM64
cross-builds passed. Those local results are preserved separately in
`../memory-linux-20261015/`.

## Final integration checkpoint: pending

Source `375f6f7625734ed7db80c62b020b6e8e32df7d82`, based on
`d8e9556dea3f1867a5af5297d1ebddbd165d9102`, also carries the newly merged
upstream task-news, Body and Touch changes. All nine Memory task
source/test/manifest files remain unchanged. Native ARM64 run 38615610362
is queued for this exact head; no Memory PR or final-head ready claim is
made until its full workspace test and artifact gate is green. The
local final-head strict lint/build checks are separate from this full
native ARM64 test run.

## Not shown

No deployment, lamp execution, flash durability, physical privacy or
end-to-end acoustic latency. ARM64 execution here was on a CI runner.
