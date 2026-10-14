# Memory compatibility with upstream Voice tasks

Ready for lamp-voice: no

Fourth's tested task source is
`52c1f698319460c4079715ba4ddf0811e804cd1a`, based on
`c2ac775f5062fb082fefb562361d75d15cadd6d0`. This follows upstream's opt-in
Voice task tools and quiet-room short utterances. Eight of the nine task
source/test/manifest files are unchanged from the failed native ARM64
checkpoint `68606e78e75369393168f184ae00ae210ecd80de`. The only difference
in the ninth, `thread.rs`, is upstream's `tasks: None` test-constructor field.
The inherited lockfile is unchanged. See `source-sha256.json`.

The subsequent local task checkpoint
`9ff0ea5b58d5a0542cc7aee90a64436d4edc71f2` folds in this evidence and
rebases onto `3773c1fb88190fb5289bf1378eaa1f5fe5d134cb`, whose new files
are First's deployment units/documentation. Crate sources, manifests,
lockfile and vendor files are identical to the tested source. This checkpoint
has not been pushed; `origin/fourth/memory-voice` remains at the failed head
above while the capture-fixture owner question is pending.

## Mac gate

macOS 26.6.2, x86_64 Intel i7-10700K, 16 logical CPUs, 64 GiB RAM,
Rust/Cargo 1.96.0. From `lampOS/`, with `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, and no competing local build/test:

```sh
cargo fmt --all --check
cargo clippy -p lamp-memory -p lamp-voice -p lamp-voice-eval -p lamp-tasks --all-targets --offline --locked -- -D warnings
cargo test -p lamp-memory -p lamp-voice -p lamp-voice-eval -p lamp-tasks --offline --locked --no-fail-fast -- --test-threads=1
cargo build --workspace --offline --locked
```

All passed, exit 0: **1,581 passed, zero failed, 11 ignored** across
68 completed suites, including doctests. `mac.txt` is the complete unedited
output; `results.json` records the counts and its SHA-256. Manual/device
cases were not executed. Tasks fixtures use local stand-ins, not an external
agent or service. The Linux-only failing capture fixture does not run here.

## Linux compilation

After the Mac gate finished, the existing x86_64 Linux 6.12.13 Podman VM
(six virtual CPUs, 8 GiB RAM; its name `lampos-arm64` is not its architecture)
ran image `localhost/lampos-fourth-native-check:latest`, SHA-256
`203a72b687c4c5a96633420af4eccf839ff34fa448a26258ca5c89a077d7e6ad`.
Networking was disabled, all capabilities dropped, UID 0, registry/git
caches read-only, four build jobs, target volume `lamp-fourth-controls-target`.
A fresh copy of 1,396 tracked files at
`/private/tmp/lamp-fourth-memory-tasks-source-m45wmjri` was mounted read-only
at `/source_memory_tasks`.

```sh
cargo fmt --all --check
cargo clippy -p lamp-memory -p lamp-voice -p lamp-voice-eval -p lamp-tasks --all-targets --offline --locked --target x86_64-unknown-linux-gnu -- -D warnings
cargo build --release -p lamp-memory -p lamp-voice -p lamp-tasks --offline --locked --target aarch64-unknown-linux-gnu
sha256sum /target/aarch64-unknown-linux-gnu/release/lamp-memory /target/aarch64-unknown-linux-gnu/release/lamp-voice /target/aarch64-unknown-linux-gnu/release/lamp-tasks
```

All passed, exit 0, no warnings. Complete output is `linux.txt`. Binaries
were cross-built, not executed on ARM64:

| Binary | SHA-256 |
|---|---|
| `lamp-memory` | `63ac25d663b539645781a526e5600ae49c74fe81997e66ddc2f76b32106993c5` |
| `lamp-voice` | `715fc2aa5886bc999fd3db89691c3b08ec501e3c1cb2ace3b60831e7404e1cdf` |
| `lamp-tasks` | `d97ee80e3d0fa7a842ac46a568f827c2aab44a182466bc47483812a405d39aaa` |

## What remains

The [native ARM64 capture setup failure](../arm64-capture-connect-20261014/)
is not cleared by these host results. The fixture is unchanged and outside
Fourth's authorized Voice scope. Its owner question remains on
[#698](https://github.com/autonomous-ai/lampOS/issues/698#issuecomment-6171516888).
Memory needs that finding addressed and a passing native ARM64 gate on
the final task head. No lamp, microphone hardware, cloud provider, model,
physical latency, or external task execution was qualified.
