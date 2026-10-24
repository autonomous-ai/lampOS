# Rust formatting in CI

The `lampOS clippy` and `lampOS on ARM64` workflows install the pinned Rust
1.96.0 toolchain with `rustfmt`. After Rust setup, each job checks the whole
workspace from `lampOS/`:

```sh
cargo +1.96.0 fmt --all --check
```

This mandatory step runs before Clippy or the build. A formatting error fails
the job; the step has no conditional or error override and does not rewrite
source files. ARM64 full-run shards perform the same check.

Formatting supplements compilation, lint and tests. It does not replace them
or establish a passing CI run. Existing workflow triggers, job names, matrix,
cache handling and artifact publication stay unchanged.
