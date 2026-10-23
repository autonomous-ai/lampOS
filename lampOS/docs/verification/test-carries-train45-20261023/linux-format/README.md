# Linux formatting on the train-45 carries

Both immutable heads passed one source-only `cargo fmt --all --check` on Linux x86:

| PR | Exact head | Exit | Container wall time |
| --- | --- | --- | --- |
| #3190 | `4b92b677d379b6ac1dadbd9e306320a04af48672` | 0 | 15.885 s |
| #3218 | `238403b1c5959fe8834aba09cfbbf24f03b6d893` | 0 | 14.554 s |

Both are parented by `8e3eaa6c26cbe1a5a0c924f5b3a7e972d9083850`. Each PR's remote head was verified before execution. Each archive contains all 6,682 tracked lampOS inputs from the named commit, with no overlays. Git head/tree/parent and archive identity checks passed, and the container's before/after source fingerprints match. Wall time includes setup, extraction, formatting and source hashing; it is not rustfmt-only time.

The unchanged approved runner used the cached rootless Linux amd64 image, Rust/Cargo 1.96.0 and rustfmt 1.9.0-stable with host `x86_64-unknown-linux-gnu`. It used `--pull=never --network=none --read-only`, a private 1 GiB tmpfs, default user 1000:1000, no host mounts or persistent/shared targets, and no added privilege/capability. Archive preparation was bounded at 600 seconds and each container at 300 seconds, with a one-use marker. No build, test, ARM, device, branch or MAIN-target action ran. No attempt was retried.

The [source-input audit](source-input-audit.json) finds that all 10,028 repository paths match between the heads outside the ten authored Voice/docs paths. All non-Voice crate inputs, 58 Cargo/toolchain paths and root robot fixtures match. #3218's Rust change is within its `cfg(test)` module. #3190's fixture behavior is test-scoped, but `stop_words::Live::hand` also introduces a non-test local alias for the same production clock function and calls that alias. Its release source/token input is therefore not literally unchanged. The full Voice fixtures differ between the heads.

These static facts do not establish current compiled-artifact identity or permit sharing required qualification. No gate was skipped or reused. Any later reuse decision needs actual current-head artifact, complete census and environment proof plus reviewer acceptance; prior-head results do not supply that proof. The clock-call alias preserves the chosen production function in source, but this audit ran no compiler or behavior comparison.

Each PR folder contains exact [source](3190/source.json), command, result, remote-head, preparation, projected image and lossless raw-stream records. [Summary](summary.json) provides both results. Full source inventories, tar archives, complete image/API responses and measuring scripts remain [external and hash-bound](external.json); no scripts, archives, binaries or account metadata are included. [Validation](validation.json) and [SHA256SUMS](SHA256SUMS) bind this compact receipt. Tests, CI status, current-parent readiness and retained failing gates remain separate.
