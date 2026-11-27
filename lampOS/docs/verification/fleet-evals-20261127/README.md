# Bounded evaluation history: host verification

Part of #8793. This checks the unused `lamp-evals` library, not a running
fleet evaluator. No deployed binary depends on the crate and no product unit,
producer switch, lamp setting or report transport changes in this piece.

## Gates run

From the lampOS workspace, on Mac and in an isolated Linux x86-64 container:

```sh
cargo test --offline --locked -q -p lamp-evals
cargo clippy --offline --locked -q -p lamp-evals --all-targets -- -D warnings
```

- Mac: 28 core tests and 13 storage tests passed; strict Clippy passed.
- Linux x86-64: 28 core tests and 14 storage tests passed; strict Clippy
  passed. The extra test refuses a writerless FIFO without a blocking read.
- `cargo fmt -p lamp-evals --check` passed on Mac.
- The Linux run used the actual workspace and pinned lockfile, with source
  and existing dependency caches mounted read-only, networking disabled,
  two CPUs and a 2 GiB memory limit. Build output stayed in the disposable
  container. The first offline attempt lacked a Git dependency cache; mounting
  the cache at the image's actual `CARGO_HOME` resolved it.

Core cases cover replay/restore, out-of-order identity ranges, complete UTC-day
boundaries, retention clipping, unknown clocks, cancellation partitions,
histogram boundaries, all capacity limits, arithmetic conservation, malformed
checkpoints and aggregate privacy. Storage cases cover complete replacement,
restored deduplication, corruption, truncation, competing writers, file types,
private modes, links, trailing-slash/dot symlink bypasses, path replacement,
crash-leftover cleanup and a reopenable directory-capacity boundary.

The review found and fixed partial-day daily output, unbounded crash-leftover
files, concurrent save/cleanup access, an inconsistent first-save capacity
boundary, and a final-directory symlink followed through trailing slash
syntax. The symlink regression failed on the original implementation before
lexical normalization was added.

## Not established by these checks

The storage fixtures simulate interrupted candidates; they are not physical
power-cut tests. No ARM64 lamp, ingestion latency under live voice load,
producer adapter, nightly check, upload, backend delivery, consent withdrawal
or fleet rollout was exercised. The proposed ingest target is under 1 ms on
the lamp and still needs measurement. Sparse/interleaved identities can
exhaust the 4096-range cap before 30 days; rejected evidence remains explicit.

Native ARM64 CI and independent review are required before this branch is
ready. Runtime integration and its own device row remain separate work. See
[the contract](../../evals.md) for exact semantics and remaining gates.
