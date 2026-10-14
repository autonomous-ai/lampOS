# Memory verification on ordinary-user native x86_64 Linux

Ready for lamp-voice: yes (#743)

Source: `56b458966ffd10a7938d551de0449cf9fbc723bd`, based on First's
requested Tasks repair `1a38f2f3e6b8afd1e7896761a0da6149a2400f67`.
`source.json` records all nine unchanged task file hashes, the lockfile,
the fresh 1,495-file archive, image identity and execution environment.
This is the local Linux VM on Fourth's Intel Mac, not ARM64 or a lamp.
The VM name `lampos-arm64` is historical; both image and kernel are x86_64.
It has six vCPUs and 8,192 MiB assigned, Linux 6.12.13, Rust/Cargo 1.96.0.

The container runs as numeric UID/GID 1000, with network disabled and all
capabilities dropped. Source and the existing offline Cargo registry/git
cache are mounted read-only. `/cargo` is an ephemeral tmpfs; only Fourth's
`lamp-fourth-controls-target` volume receives build output. No account or
credential was created. The image derives from Fourth's existing local
toolchain image; it makes that image's `/root` traversable and selects the
installed compiler directly. It does not change the host's `/root`.

## Disk-full attempt, no tests executed

Formatting and strict workspace Clippy passed. The subsequent debug test
build failed with exit 101 before any test ran: rustc reported `No space
left on device`, then linkers reported ENOSPC and bus errors. Neither the
workspace build nor ARM64 cross-build was reached. `space-failed.txt` is
the complete unmodified output; `space-failed-results.json` records its
hash and terminal status.

The VM filesystem was 100% full with only 153,845,760 bytes available at
the cleanup boundary. Its native debug cache was 26,871,489,020 bytes
(apparent size); memory had approximately 7.4 GB available at the separate
post-failure diagnostic boundary. `link-diagnostics.txt` and
`cache-before.txt` preserve those readings. Disk exhaustion is directly
reported by the compiler; no claim of an out-of-memory failure is made.

After the gate exited and no container was using the volume, Fourth removed
only `/target/x86_64-unknown-linux-gnu/debug` in its dedicated cache. Source,
raw evidence and ARM64/release caches were untouched. Available filesystem
space then rose to 26,684,542,976 bytes; `cache-cleanup.txt` records both
boundaries. Apparent cache bytes and reclaimed filesystem bytes are distinct.

## Gate after the environmental repair

The fresh run keeps the exact source and all test assertions. It sets
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` solely to omit
debug symbols and fit the generated workspace artifacts in the 40 GiB VM.
Optimization and debug-assertion settings are unchanged. It otherwise
uses `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, and
runs sequentially from the read-only archive's workspace root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline --locked --target x86_64-unknown-linux-gnu -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast --target x86_64-unknown-linux-gnu -- --test-threads=1
cargo build --workspace --offline --locked --target x86_64-unknown-linux-gnu
cargo build --release -p lamp-memory -p lamp-voice -p lamp-tasks --offline --locked --target aarch64-unknown-linux-gnu
sha256sum /target/aarch64-unknown-linux-gnu/release/lamp-memory /target/aarch64-unknown-linux-gnu/release/lamp-voice /target/aarch64-unknown-linux-gnu/release/lamp-tasks
```

The test build completed after the environmental repair. `after-link.txt`
records 3,276,134,187 apparent bytes of native debug output and
23,412,469,760 filesystem bytes available before the remaining test run.
The run completed with exit 0: **3,995 passed / 0 failed / 32 ignored,
269 suites**, including the custom turn-end-wire harness. Formatting,
strict workspace Clippy, workspace build and the three release ARM64
cross-builds passed. The unchanged Voice library reported 863 passed /
0 failed / 6 ignored in 400.63 s, including all three Voice cases from the failed
Mac run. The previously failing Tasks harness case also passed in its
own crate. This is separate Linux evidence, not a reversal of the Mac result.
The complete output is `56b458966-gate.txt`; counts, terminal status and
binary hashes are in its adjacent JSON files. Its raw SHA-256 is
`d94c58f65a57abfb1101993426016874bae37f9316963ac25962c36b5a5d26b2`.
The inherited Controls/Expression `replay_cost` output-name collisions
remain in the raw test output; strict Clippy itself passed.

## Final integration checkpoint

The task was then rebased to `375f6f7625734ed7db80c62b020b6e8e32df7d82`
on `d8e9556dea3f1867a5af5297d1ebddbd165d9102`, so the handed-back Voice
binary also includes the newly merged upstream task-news, Body and Touch
changes. All nine task source/test/manifest files remain unchanged;
`375f6f762-source.json` records their hashes and the new 1,654-file source
archive. The lockfile change belongs to the upstream Touch dependencies:
`cargo fetch --locked --target x86_64-unknown-linux-gnu --target
aarch64-unknown-linux-gnu` fetched i2cdev 0.6.2 and nix 0.26.4; the raw fetch
output is retained.

The first final-head attempt passed formatting, strict workspace Clippy
and the native workspace build, then failed ARM64 cross-compilation with
four unresolved-import errors for Touch types. No source change was made:
the archived `msgs/src/controls.rs` contains those types and matches Git
(SHA-256 `2c8c2644141d1beb01c0f9c70817afbf09327c5eb736704856a13d632196e3f1`).
The source file's timestamp was 18:31:00 UTC, earlier than the old-source
ARM64 `lamp-msgs` dep-info timestamp of 18:34:44 UTC: the earlier gate
finished after the newer source archive was created. Cargo reused that
older-source library from the shared target cache. The raw failed output,
exit 101 and timestamp diagnostic are retained in the `375f6f762-cached-`
and `375f6f762-cache-` files. This is Fourth's local cache issue, not an
asserted upstream Touch defect.

Fourth then used `cargo clean -p <each workspace package>` for the native
target and `cargo clean --release -p <each workspace package>` for the
ARM64 target, both in its own cache. Third-party dependency artifacts and
source remain intact. All final-head formatting, strict workspace Clippy,
native workspace build and the three ARM64 cross-builds then passed,
exit 0, with identical source in the same ordinary-user offline environment.
The cleaned native workspace artifacts occupied 3.2 GiB (4,708 files), and
ARM64 release workspace artifacts 252.6 MiB (296 files), as Cargo reported.
The successful rebuild of unchanged source confirms the stale-cache
failure. Complete output and binary hashes are in `375f6f762-fresh-build.txt`
and `.json`. The strict lint/native build/ARM64 build reported 35.27 s,
20.65 s and 48.84 s respectively; these are build elapsed times, not
runtime or lamp latency. The final-head full test suite is
native ARM64 run 38615610362. The complete local test result above remains
attributed to 56b458966; it is not relabelled as a run of the final head.
That exact-head native ARM64 run subsequently passed 4,245 / 0 failed /
32 ignored across 280 suites, with the release build, hashes and artifact
upload successful. Its complete evidence is in `../memory-arm64-20261015/`;
Memory is offered as non-draft PR #743. The older local full-test result
remains separately attributed to its actual source.

## Not shown

No lamp execution, physical latency or deployment. Cross-compilation is not
ARM64 execution. The prior complete Mac gate remains failed with four cases
and 20 isolated follow-up passes, preserved in `../mac-workspace-20261015/`.
Raw `.txt` whitespace is intentionally preserved.
