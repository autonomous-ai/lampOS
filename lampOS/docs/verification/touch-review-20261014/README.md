# Touch review after rebase — 2026-10-14 UTC

Exact tested Git head: `a7c0073da9b2314ba123efee8a25bc80ce01b211`, on base `d5d765862ab1f73629ee7a2c5a9a8086f0246d11`. [Native run 38599296426](https://github.com/autonomous-ai/lampOS/actions/runs/38599296426) completed **successfully** on Ubuntu 22.04 ARM64 with Rust 1.96.0. This is a CI machine, not a lamp.

## New-head evidence

- Darwin x86_64 host, Rust 1.96.0: whole-workspace format; strict all-target Clippy for 21 touched/reverse-dependent crates (including newly upstream Mood); **446 Controls/Msgs tests passed, 0 failed, 0 ignored, 29 harnesses**; workspace build. Host source was clean and all 808 captured source hashes were unchanged. Source cwd was `/private/tmp/lamp-third-touch-review/lampOS`; shared build cache was `/Users/d/code/autonomous-os/lampOS/target`. Command elapsed times are not interaction latency.
- Native release build: `cargo build --locked --release --workspace --bins --examples`, from `lampOS/`.
- Native complete workspace tests: `cargo test --locked --release --workspace -- --test-threads=1`, from `lampOS/`: **4,192 passed, 0 failed, 31 ignored, 276 standard libtest harness summaries** (0 measured; 0 filtered out). The separate `lamp-turn-end-wire/tests/process.rs` runner (`harness=false`) reports another **12 passed / 0 failed**, outside those standard totals. This supplies the complete dependent-crate tests; it is not a Controls-only run. Host and native counts overlap and are not added.

New-head default-off replay also passed after an explicit Controls rebuild in the isolated worktree. The private Darwin debug binary SHA-256 was `0292b61a46df9f73ba7203725be84ea8ffd5ff3d2d2c1f74e54f9bf1d6ab8db0`. Without `--touch`, shadow stdout (76,658 bytes) and legacy-enabled stdout (76,392 bytes), plus both stderr streams (134 bytes each), are byte-identical to the archived outputs previously proven identical to pre-task baseline `d672844cceaa9ff5488c0c0a42c8fcbdc0df4478`. Both legacy input Git blobs match that baseline. `lamp-third-touch-review-default-off-results.json` records exact arguments, compiler-artifact provenance, binary/input hashes, raw filenames and exit codes; source hashes remained unchanged. This replay opens no hardware or bus.

Native warning audit: two Cargo output-filename-collision warning headers for `replay_cost` and `replay_cost.dwp` during release build, repeated in the test stage; zero Rust source diagnostic warnings. Setup also emits two rustup warning lines and six Actions Node deprecation warnings. See `lamp-third-touch-review-arm64-warnings.txt` and `-count-check.json`. The native workflow does **not** run Clippy. The two additional ignored cases relative to the old run are upstream Mood's manual timing probes; all ignored cases are retained in the result metadata.

Only `docs/reuse-decisions.md` required manual conflict resolution. Touch and Mood research/limitation text were both preserved. All Controls files are identical to the old Touch head `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`; Msgs retained the Touch contract and gained only upstream Mood's module/export and new file. No Touch runtime fix was made during this rebase.

## Raw evidence

- `lamp-third-touch-review-arm64-full.txt`: complete unmodified GitHub run log; SHA-256 `47b274221fda6b6c11915a19da4377a2f5a86a1d62c3043cb329274e7fb539e3`.
- `lamp-third-touch-review-arm64-run.json` and `-status-final.json`: terminal run/job/step metadata tied to the exact Git head.
- `lamp-third-touch-review-arm64-harnesses.json` and `-results.json`: individual raw-line references and summed harness counts.
- `lamp-third-touch-review-arm64-SHA256SUMS`: 33 binary hashes extracted from the run's “SHA-256 of each binary” log step.
- `lamp-third-touch-review-arm64-artifacts.json`: artifact metadata, separate from the per-binary hashes. Artifact `lampos-arm64-a7c0073da9b2314ba123efee8a25bc80ce01b211`, ID `11914551344`, archive digest `sha256:d6c8da10177859085d20ba92cb43f1afba7e894666a8924693e42e75d0c44229`, size 56,705,466 bytes, expiry **2026-10-21 17:01:30 UTC**. No binary archive was downloaded for this review.
- `lamp-third-touch-review-arm64-commands.json`: exact read-only collection commands and exit codes. Initial/final status and watcher output are also retained.

New Controls **binary SHA-256**: `8c7b8a1a6c3437853d616ae01244042057800a5a6bbc540006d78ce730b56ceb`. This is not a Git commit.

## First's new-artifact lamp check

Use Controls from artifact `lampos-arm64-a7c0073da9b2314ba123efee8a25bc80ce01b211`, ID `11914551344`, before its **2026-10-21 17:01:30 UTC** expiry. Do not substitute a local rebuild. These commands are instructions for First; Third has not run them on a lamp.

From the extracted artifact directory, verify the binary:

```sh
printf '%s  lamp-controls\n' '8c7b8a1a6c3437853d616ae01244042057800a5a6bbc540006d78ce730b56ceb' | sha256sum -c -
```

First supplies the current verified **legacy** Controls configuration and pins files, and an isolated bus path through the named variables below. Stop the previous GPIO/I2C owner under the lamp's existing procedure before starting a replacement; preserve it as rollback. Keep the installed legacy `--enable` choice; only the new `--touch` option must be absent. This command shows the shadow form; add `--enable` if that is how the existing service runs, and record the actual arguments:

```sh
./lamp-controls serve \
  --config "${LEGACY_CONTROLS_CONFIG:?set the verified legacy config path}" \
  --pins "${LEGACY_CONTROLS_PINS:?set the verified legacy pins path}" \
  --bus-root "${ISOLATED_CONTROLS_BUS:?set the isolated bus path}" \
  --seconds 60
```

Do not pass the new touch pins schema to this legacy check. Afterward use the per-unit qualification sequence in `docs/controls.md`; the example thresholds are not calibration. The rebased binary has not been run on a lamp by this review.

## Earlier evidence remains separate

The old Git head `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca` has its existing preserved host, source-only and native evidence: host 3,283 passed / 15 ignored across 204 harnesses, source-only Controls/Msgs 446 passed / 0 ignored across 29 harnesses, and native [run 38573575450](https://github.com/autonomous-ai/lampOS/actions/runs/38573575450) 4,054 passed / 29 ignored across 264 harnesses. Earlier development failures and the unchanged inherited Mac motor-cleanup timeout remain in the prior evidence folders; they were not removed or relabeled.

First's [PR comment](https://github.com/autonomous-ai/lampOS/pull/728#issuecomment-6172695270) reports one minute on **lamp-4ace**, from 23:04, using old Controls binary SHA-256 `37924f2bfea364a214243e7b935a85ca1e8302167393c34590530a2908fc18f1` from old Git head `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`: no `--touch`, no restart, Controls `ok`, “head not touched”. This is First's lamp measurement. It does not qualify the new binary or tap/hold/slide behavior.

What has not been shown: the rebased binary on the lamp, per-pad threshold calibration, physical contact or actuator latency, electrical contention behavior, enabled final-owner Sleep/Wake/Stop, or quiet-room gesture UX. First still owns those device checks. Excluded E4/E8 are a measured lamp-4ace issue; other lamps require their own working electrodes and per-pad thresholds.
