# Stable promotion: local verification, 25 October 2026

The implementation was tested on a native ARM64 Mac with Rust 1.96.0,
based on `d7d20bfc0`. The standalone CI tool depends on the updater library;
it adds no product binary or runtime switch. Registry dependency versions
match the existing workspace lockfile.

Commands, from the repository root, with one compiler job and an isolated
target directory:

```sh
cargo +1.96.0 fmt --manifest-path .github/tools/lampos-promote/Cargo.toml --check
cargo +1.96.0 clippy --offline --locked --manifest-path .github/tools/lampos-promote/Cargo.toml --all-targets -- -D warnings
cargo +1.96.0 test --offline --locked --manifest-path .github/tools/lampos-promote/Cargo.toml
python3 -B -I .github/scripts/lampos-publish-test.py
```

Formatting and strict Clippy passed. Rust reported 2 library tests,
22 promotion tests and 1 workflow test passed, with zero failures or
ignored tests; binary and doc-test targets contained zero tests. The
existing publisher suite reported `Ran 35 tests` and `OK`. Both changed
workflow files parsed as YAML, and the diff whitespace check passed.
SHA-256 inventories of every standalone tool source, manifest and lockfile
matched before and after the successful gate. No new tool symbol occurs
under `lampOS/crates`; no runtime crate source was changed.

The first compiler attempt failed with E0282/E0283 at the swap closure.
An explicit `Result<()>` annotation fixed it before the successful strict
Clippy/test run. An independent source review also found a test fixture
missing the required setting mode; `0644` was added before any test run.
These are separate observations, not passing results for the earlier source.

Controls cover exact signed-byte preservation, trusted-key and source
run/attempt checks, payload corruption, monotonic selection, identical
retries, unsupported external models and future updater levels, missing or
ambiguous heads, upload failure before/after the remote write, staged
corruption, both rename failures before/after their remote effects, failed
readback/restoration, and competing canonical assets. A fixed disposable
test key and simulated GitHub responses drive these controls.

Read-only public API checks confirmed that release 6819 records source
`22d93d199`, run `40189167200`, attempt 1, and that its run and both required
jobs expose the fields and success values the adapter checks. That metadata
check did not execute the promotion tool against GitHub or qualify the
release's real signature with a provisioned key.

Native Linux ARM64 and x86 Linux Clippy are the PR's separate gates. This
local record does not claim their result, a real stable promotion, trusted
key provisioning, product-feed activation, or a lamp update. Those actions
were not performed.
