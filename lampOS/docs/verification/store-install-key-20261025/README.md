# Store installation and its separate key, 25 October 2026

Release task: [Queue assignment](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6394656953).
Store stays off. No key, activation, lamp run or device latency result is claimed.

The local gates ran on the development Mac, Rust 1.96.0, native x86_64,
from `lampOS/`; the Rust gate base is `b241d0207`. One compiler job and an exclusive fresh
target directory; runtime tests use `RUST_TEST_THREADS=1`.
[source-sha256.json](source-sha256.json) records the 19 task inputs.

Commands and results:

- `cargo +1.96.0 fmt --all --check`: passed.
- `cargo +1.96.0 clippy --locked -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-peek -p lamp-setup -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings`: passed.
- `cargo +1.96.0 build --locked --workspace`: passed. The resulting update
  and skills binaries are Mach-O x86_64; no ARM64 link or execution on Mac.
- `cargo +1.96.0 test --locked --no-fail-fast -p lamp-update -p lamp-skills -p lamp-cloud`: **1,110 passed, 0 failed, 19 ignored**, exit 0 (21 Rust harness summaries).
- `cargo +1.96.0 test --locked --no-fail-fast -p lamp-body -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-peek -p lamp-setup -p lamp-voice -p lamp-voice-eval`: **6,230 passed, 0 failed, 33 ignored**, exit 0 (58 Rust harness summaries).
- From the repository root, `bash -n lampOS/deploy/factory/steps.sh lampOS/deploy/factory/steps-test.sh`: passed.
- `bash lampOS/deploy/factory/steps-test.sh`: **84 passed, 0 failed**,
  [raw output](factory.txt). Temporary fake devices and services only.

The new tests cover separate-key readiness, missing and malformed keys,
no fetch while refused, removal without a usable key, installer defaults,
preserved local choices and reload recovery. Factory fixtures cover dry
provisioning, optional absence, repeated keys, replacement refusal, malformed
and wrapped base64, destination links and missing arguments.

The installed helper sandbox prepares network permission even while off;
the helper constructs no HTTP client until its switch and key checks pass.
The Store files remain installer settings outside the current OTA settings
allowlist. Public keys are never supplied by a build manifest.

Native Linux ARM64 and Linux strict clippy are the pull request's CI gates.
Their exact runs and current results belong in the PR body before readiness.
