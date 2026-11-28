# Sensor 4 isolated facing observation: verification

Status: not qualified for the combined head-and-shoulders job. This is an off-by-default measurement candidate on exact `first/simple-call` base `0dbbfc95bbea6ddf397308e1a8ef1933c5d19dcb`, with the prior #8856 geometry and #8865 source-freshness guard reused. It does not change voice admission, legacy face fields, product units, gaze, camera cadence or inference calls. No device access, install, merge or train was performed by this builder.

## Code gates

Rust 1.96.0, actual lampOS workspace root (required for the camera UAPI build input), offline and locked. The standalone-package attempt was refused before tests because it lacked that workspace configuration; it is not gate evidence.

- Mac: `cargo +1.96.0 test --offline --locked --release -q -j 2 -p lamp-eyes`: 567 passed, 6 ignored, 0 failed.
- Mac focused `head_direction`: 48 passed, 1 ignored. Example `head_direction_compare`: 8 passed, 0 ignored.
- Mac: `cargo +1.96.0 clippy --offline --locked --release -q -j 2 -p lamp-eyes --all-targets -- -D warnings`, release example build, and workspace formatting passed.
- Linux x86-64 container, source mounted read-only, network disabled, 2 CPUs/4 GiB: `cargo test --offline --locked -q -j 2 -p lamp-eyes`: 569 passed, 6 ignored, 0 failed. Strict all-target Clippy and formatting passed. Source hashes remained unchanged throughout the 619.43-second gate.
- The ignored model/runtime, people-fixture and timing cases are not presented as device passes. The new classifier measurement was explicitly run in a separate optimized host harness; no new real-person fixtures were created.
- Native ARM64 CI and holder-reported file timings are separate evidence; their completed results and provenance limits are recorded below.

Ten source hashes, exact commands and logs are retained in the local `native-source-v1.json`, `native-gates-v1.json` and `linux-gates-v1.status.json`. The compiled host replay executable is retained, SHA-256 `00bb1ab935ae94820549bd6c6a191a3d7ba4943475f4572e2ec7927c7b233e9d`. It is not an ARM64 executable.

## Isolation, privacy and timing

Independent source review checked all three observer-local main-loop call changes against #8865, per-method count conservation, final permission/source-age validation before counting, saturation, closed serialization and working/full crop selection. Geometry and freshness source are unchanged from #8865. The observer never retains or serializes face identity, images, landmarks, angles or per-look timestamps; it retains fixed aggregate counts. Shoulders remain explicitly unknown because no fresh same-person orientation signal is associated.

The added-classification target was below 1 ms per look. Optimized standalone host elapsed measurement (legacy yaw plus three fixed classifiers): first 1.045 microseconds; 1,000 repeated samples median 0.087, p95 0.125, maximum 0.713 microseconds. This excludes model inference, mesh geometry, guard/report work and scheduler effects. It is not measured CPU time, full interaction latency or board capture latency. The on-board p95 source-capture-to-result target remains unmeasured.

## Replay helper controls

The [offline replay helper](../../../tools/head-facing-eval/README.md) validates the exact product model hashes, records input/executable/output hashes, refuses an existing result directory and preserves nonzero producer results. The default uses the prior reviewed working-crop producer; the new optional full crop uses the same decoded source frame with the existing crop function. Both report file-read-to-result timings, with capture age null.

Linux synthetic wrapper fixtures passed with GNU timeout: default and explicit full crop, bare executable-name resolution, existing-output refusal, nonzero exit/output preservation, exact model-hash refusal and private result-file modes. Mac Bash 3.2 exposed two real portability failures: expanding an empty array under nounset and BSD chmod's interpretation of an option after the mode. Both are fixed. The exact old script and failure logs remain retained. Default/working/full Mac wrapper fixtures now pass; the Mac fixture uses an explicit timeout-dispatch shim because GNU timeout is absent there, so it establishes shell compatibility, not timeout enforcement. No test claims timeout-expiry or physical-lamp behavior.

## Measurement evidence

The [evaluation document](../../head-direction-evaluation.md) records the historical 2,790-image angle benchmark, the frozen three-rule binary proxy, unknown coverage and insufficient small-face support. All public image hashes match; eight synthetic analysis checks and 16,740 actual Rust classifier comparisons pass. The original observations, analysis, all reruns, errors and README files are retained. No thresholds were optimized from these results.

Frozen scorer v2 `31c290d5bb9dc845679098a837a4d0ffd5059cb4` requires independently labelled head and shoulders, real-person holdout, verified clocks and each class/disagreement quota. Its shipped invented adapter fixture was validated and then correctly returned NOT QUALIFIED. That fixture is a contract check only; neither it nor the public pose data supplies missing shoulder gold, physical distance or capture clocks. The joint sensor remains unqualified and cannot join the combined judge.


## Completed CI and separate development calibration

Exact compiled source `77bea27d8e8207c6c41ed56a11d3f3a0aa4a30cf` passed [native ARM64 build/tests](https://github.com/autonomous-ai/lampOS/actions/runs/46031285290) and [whole-workspace strict Clippy/formatting](https://github.com/autonomous-ai/lampOS/actions/runs/46031356573). Promotion and publication were skipped. The [independent source/privacy read](https://github.com/autonomous-ai/lampOS/pull/8932#issuecomment-7223235011) passed; its qualification hold remains. Subsequent documentation corrections and evaluation results do not change compiled code.

The [one bounded calibration round](../../head-direction-evaluation.md#one-bounded-development-calibration-round-2026-11-28) is retained separately from the fixed v1 rules. It evaluated 3,564 variants once with exact Rust f32 arithmetic and kept all input/source evidence. Primary participant-out development recall is 757/810 inside and 1,224/1,980 outside: neither reaches 95%. Independent verification checked all 46 reported choices and the saved predictions/transitions. The all-data fitted parameters are explicitly optimistic, not a runtime change or an independently qualified sensor. No new device or latency measurement was made.

## Review of the lead's retained measurements

The [person-split review](../../head-direction-evaluation.md#reviewed-person-split-and-angle-diagnostics-2026-11-28) reproduces the lead's locked-cone test counts: 389/432 inside and 788/1,056 outside. Both bars fail. Both image series stay with their person, but all 15 people were previously used for development. The review checked retained hashes, split/selection code and numeric outputs and added descriptive pan/tilt plots; it did not repeat fitting, inference or training. The scripts' overwrite behavior and two unpinned comparison inputs are explicitly recorded, not treated as enforced once-only execution.

The [holder file-replay review](../../head-direction-evaluation.md#reviewed-holder-file-processing-replay-2026-11-28) validates 18 completed rows and matching recorded image digests. First processing is 221.892 ms; the following 17 rows have median 153.408 ms and nearest-rank p95 169.200 ms. Executable-to-source provenance remains pending, capture latency is unmeasured, and no device connection was made by this builder. Neither result removes the qualification hold or enables the observer.
