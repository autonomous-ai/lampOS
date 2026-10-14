# Memory adapter documentation: exact-head native ARM64 gate

Documentation-only follow-up to the request on PR #743. The only changed
path from `f60d6d6f9c7895c84e127cb6c1172eef46148992` is
`lampOS/docs/memory-days.md`: it explains which capabilities remain when the
model adapter is absent and why Voice and Memory must be upgraded together.
`change.patch` and `source-validation.json` retain that scope and document
hash. No runtime, test, dependency, workflow, switch or deployment file changed.
Local source review and `git diff --check` passed on Fourth's Intel Mac.

[Native ARM64 run 38629635936](https://github.com/autonomous-ai/lampOS/actions/runs/38629635936)
succeeded at exact head `c98a5b05056d1f6e27b425f917a853361bfac754` on
`fourth/memory-adapter-docs`. It ran as an ordinary user on GitHub's Ubuntu
22.04 ARM64 runner with Rust/Cargo 1.96.0. This was not a lamp.

The workflow's relevant commands, from `lampOS/`, were:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

Both passed, as did binary hashing and artifact upload. The run was created
at 20:09:13 UTC on 2026-10-14 and completed at 20:39:49 UTC. Audited totals:
4,245 passed / 0 failed / 32 ignored in 280 suites: 4,233 standard
libtest/doctest passes plus the separate custom harness's 12 passes.
`counts.json` distinguishes these groups. `raw.txt` is the full unmodified
GitHub output; its SHA-256 is
`991e691413493771cd19d30cd74bff77f712bbe3a3ca46b0ba32236084b68e29`.
`run.json`, `watch.json` and `artifacts.json` retain terminal state and source
identity. `binary-hashes.json` transcribes the successful hashing step.

[Artifact 11927212131](https://github.com/autonomous-ai/lampOS/actions/runs/38629635936/artifacts/11927212131)
contains the workflow binaries, retained by GitHub until 2026-10-21. The
artifact metadata records archive digest
`sha256:d3ef8a85b811f3afc1945f27f413136adc4eb1bc07bbeb950a7ee975c20bf0ae`.
Nothing was downloaded onto or installed on a lamp for this documentation
follow-up. These binaries are tied to the stated base, not a later
integration head; this change does not request a runtime rollout.

Not shown: a configured production model adapter, automatic extraction,
semantic truth of proposed facts, physical power-loss behavior or lamp
latency. First's reported lamp checks remain attributed to PR #743 rather
than presented as measurements made here.
