# Requested recording voice/state regression — 22 October 2026

Prepared on `bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0` as a separate follow-up to [Music #1757](https://github.com/autonomous-ai/lampOS/pull/1757). Its PR waits for Music to merge. This receipt records the host checks for the two-file follow-up; it does not qualify a device, motion, acoustic output, the full coordinator loop, latency or an ARM64 run.

The production change extracts the existing owner-phase and `unshown()` filtering into private `tell_voice_of_turn`. The runtime mapping is unchanged. The new `requested_audio_keeps_voice_state_non_resting_until_final_retirement` test calls that same helper with the existing admitted owner, Skills socket fixture, speaker IPC receipts and a real `BusOut`/bus. It reads the published `voice/state` at eight lifecycle boundaries: Thinking when prepared and first dispatched; Speaking after partial/full first acceptance and while the final chunk is outstanding or accepted but unretired; Thinking after retirement while the turn remains owned; Resting only after owner completion. It checks original-owner identity and the completed Skills outcome. These are authored fixture receipts, not a full service-loop or device run.

MusicSway and LaughGlance require Resting, so a false Resting state would weaken their guard. The test does not enable either skill or change readiness, authorization, deadlines or the ring's intentional listening appearance during a recording. It adds no sleeps or timing relaxation.

## Results

| Stage | Exit | Passed | Failed | Ignored | Filtered | Command seconds |
|---|---:|---:|---:|---:|---:|---:|
| positive | 101 | — | — | — | — | 0.362 |
| positive-host | 0 | 1 | 0 | 0 | 3183 | 47.521 |
| mutant-host | 101 | 0 | 1 | 0 | 3183 | 36.345 |
| restored-host | 0 | 1 | 0 | 0 | 3183 | 36.238 |
| fmt | 0 | — | — | — | — | 4.861 |
| clippy | 0 | — | — | — | — | 33.016 |
| tests | 0 | 3445 | 0 | 14 | 0 | 406.973 |
| skills-consumer | 0 | 1 | 0 | 0 | 260 | 28.032 |
| ears-consumer | 0 | 1 | 0 | 0 | 66 | 32.184 |

`positive` preserves the initial cache-permission failure: Cargo could not open its target build lock, before compilation or test execution. It is not counted as a test failure or discarded. The later host executions are distinct attempts. The required negative control adds only `RequestedAudio` to `unshown()` and fails the first recording-state assertion: actual Resting, expected Thinking. [The exact control patch](unshown-mutant.patch) is evidence only; `notices.rs` was restored byte-for-byte, and the restored test passed. [Control evidence](control.json) records exact counts, durations, commands, source hashes and all four original raw-log hashes.

[Gate evidence](gate.json) records every stage's command, exact counts, command duration, original raw-log hash and the 333 unchanged source hashes checked for the Voice, Skills and Ears Rust/manifests scope. Voice tests ran unfiltered; the two consumer tests ran by exact name. Ignored tests remain reported. This was not a full workspace test run. Command durations include any compilation and are not response-time measurements. Original logs remain retained separately; this compact bundle records their hashes without duplicating private log content.

## Commands

Run from `lampOS` with Rust 1.96.0, offline/locked Cargo tests and Clippy, two build jobs, incremental disabled, and dev/test debug information disabled. Tests are serial. JSON files preserve the explicit environment overrides; the host executable and target paths are normalized, and unrelated inherited environment values are omitted.

```sh
cargo +1.96.0 test --locked --offline -p lamp-voice --lib coordinator::tests::requested_audio::requested_audio_keeps_voice_state_non_resting_until_final_retirement -- --exact --test-threads=1 --nocapture
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --locked --offline -p lamp-voice --all-targets -- -D warnings
cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-voice -- --test-threads=1
cargo +1.96.0 test --locked --offline -p lamp-skills --test it skills::every_skill_folder_passes_its_own_recorded_tests -- --exact --test-threads=1
cargo +1.96.0 test --locked --offline -p lamp-ears --lib classes::tests::a_window_the_lamp_spoke_into_is_its_own_and_the_next_is_unknown -- --exact --test-threads=1
```

## Source identity

The changed files are `coordinator.rs` and its requested-audio test. The third hash binds the restored, unchanged mutation target:

- `lampOS/crates/voice/src/coordinator.rs`: `902484349e2ed5282d42e097b6c8acf3bdf407fee239871c6b176df28aa479b1`
- `lampOS/crates/voice/src/coordinator/tests/requested_audio.rs`: `9fb366b4edf2958d90f15fbb0df0ea23bbc94c3f36ddf1db6abafca77bd0681c`
- `lampOS/crates/voice/src/coordinator/notices.rs`: `dec9fe70db0f6e3fa40b02344af8a38d6e2e75e7063feac301a532d26275d89d`

[SHA256SUMS](SHA256SUMS) covers this README, both public JSON receipts and the control patch. No runtime feature enablement, device action or remote publication follows from this proof.
