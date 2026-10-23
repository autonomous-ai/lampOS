# Surround status-lock fixture, 23 October 2026

This addresses only the Surround entry in [#1339](https://github.com/autonomous-ai/lampOS/issues/1339), under the [recorded claim](https://github.com/autonomous-ai/lampOS/issues/1339#issuecomment-6354242978). The missing-executable case in Turn-end-wire is outside this change. No product Rust file changes.

The candidate is the two-file test/documentation delta on `9b6960e61e5758bb745c6e4d7ad043cc725cd018`. All 6,515 tracked lampOS files were frozen, hashed and given fresh mtimes before Cargo used the shared target. The content hash was unchanged by that refresh. The test and product paths audited at `ae4ab0022` are unchanged at this base. [Source proof](source-proof.json) records the baseline, candidate, each controlled source and every exact restoration.

The test now observes status availability inside `PendingAdapter::poll`, acknowledges that observation, and checks the status again between polls. `try_read()` fails immediately if a status writer prevents access. The first-poll event has a ten-second fixture watchdog. The original Working/Failed and single-submission assertions remain; advancing the injected clock by 46 seconds must still finish within the original one-second watchdog. The product's 45-second join deadline, ten-second network timeout and 20 ms poll interval are unchanged. No stopwatch threshold was widened.

## Fixed controls

Each row ran exactly once in a separate test process, with compilation excluded from the reported wall time. Every compiled integration binary listed 263 tests, including exactly one matching test. [Commands](commands.json), [artifact identities](artifacts.json), [raw outputs](raw/controls.txt) and [control patches](controls/) retain the observations.

| Control | Result | Wall seconds | Checked diagnostic |
|---|---:|---:|---|
| Original fixture, 400 ms host pause after stopwatch start | 0 passed / 1 failed, exit 101 | 0.667470 | Original `<20 ms` assertion |
| Corrected fixture, same 400 ms pause before main status observation | 1 passed / 0 failed, exit 0 | 0.732580 | Original Working/Failed/count assertions |
| Hold the actual product status write lock around `network.poll()` | 0 passed / 1 failed, exit 101 | 0.371010 | In-poll `try_read`: WouldBlock; then closed acknowledgement |
| Remove the product logical-deadline check | 0 passed / 1 failed, exit 101 | 1.270774 | Original one-second watchdog: Elapsed |

The lock mutation releases its guard before an await, so it compiles and isolates the claimed lock property. The deadline mutation leaves the separate ten-second network timeout intact. Each mutation was restored to the exact candidate content hash before proceeding. Patches were independently applied outside the repository and matched the compiled control hashes. These controls establish sensitivity to the claimed defects; they do not establish the cause of any earlier unscheduled #1339 failure.

An initial setup attempt could not execute `rustc` because the runner inherited a PATH without the installed Rust tools. [Its failure record](initial-setup-failure.json) is preserved. No Cargo command or test ran in that attempt. Adding the installed tool directory allowed the still-unrun fixed observations to proceed; no test was retried for a passing result.

## Final gates

Rust 1.96.0 reported `x86_64-apple-darwin`; the tested binaries were Mach-O x86_64 and `uname -m` reported x86_64. Physical hardware and translation status were not established. Cargo used four jobs, debug information disabled for dev/test, incremental compilation disabled, the SMALL target, and offline mode. `RUST_TEST_THREADS` was unset. The target and CPU were assigned exclusively for these runs; this is not a claim of an otherwise idle host. See [environment](environment.json) and [toolchain output](raw/toolchain.txt).

Cargo metadata found no reverse dependents of Surround. The prescribed commands ran sequentially, with full output in [native gates](raw/native-gates.txt):

| Command | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all --check` | exit 0 | 13.503010 |
| `cargo clippy -p lamp-surround --all-targets --offline -- -D warnings` | exit 0 | 27.709945 |
| `cargo test -p lamp-surround --offline` | 265 passed / 0 failed / 0 ignored: 2 library, 263 integration; binary/doc groups had 0 tests | 167.353925 |
| `cargo build --workspace --offline` | exit 0 | 79.270004 |
| `git diff --check` | exit 0 | 4.172897 |

No warning was printed by these native gates. Final binary paths came from the test command's output; their recorded mtimes precede command completion, so the later workspace build did not overwrite them before hashing. The final integration binary SHA-256 is `f0a04951fe4f18e0ef35898197d4d2ba18ed8c9faabb710b996dc9cc6e7a6ad9`.

Linux formatting also passed in the pre-existing cached x86 image, with network disabled and no compilation or shared target. Its immutable archive included every tracked source file and both uncommitted changes, with the exact candidate aggregate hash; [snapshot proof](source-proof.json) and [raw Linux formatter output](raw/linux-fmt.txt) record this. This is not Linux Clippy, ARM64 qualification, or device testing. Those CI gates remain for publication.

Raw wrappers retain each stdout/stderr byte sequence, including blank final lines, between markers with its byte count and SHA-256. Bulk Cargo JSON, full inventory streams and the source archive remain outside the repository; [external index](external-index.json) records their hashes, sizes and explicit availability limitation. No script or binary is included.
