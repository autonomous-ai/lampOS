# Requested recording voice/state: rebase after Music merged

Follow-up head `4e060a224cb3d04bcd4404355aa5fc0bac187705` is based on the actual
[Music merge](https://github.com/autonomous-ai/lampOS/commit/2d1d353965b6b9f9bdc043ee4d19832630b3a6d8)
`2d1d353965b6b9f9bdc043ee4d19832630b3a6d8`.
[PR #2879](https://github.com/autonomous-ai/lampOS/pull/2879) is open.
**Positive ARM64 qualification and the ARM64 `unshown()` mutant check are
not yet qualified.** No native completion is claimed.

The regression exercises the production `tell_voice_of_turn` helper through a
real `BusOut` and retained bus messages, with the admitted owner, Skills socket
fixture and speaker IPC receipts. Its eight assertions require Thinking when
prepared and first dispatched; Speaking after partial and complete first-chunk
acceptance, while the final chunk is outstanding, and after its acceptance
before retirement; Thinking after retirement while ownership remains; and
Resting only after owner completion. It also checks original-owner identity
and the completed Skills outcome. The helper extracts the existing mapping
without changing its semantics. The test protects the Resting guard used by
MusicSway and LaughGlance without enabling either skill or changing readiness,
authorization, deadlines or the ring's recording appearance.

[identity.json](identity.json) and [range-diff.txt](range-diff.txt) record an equal
follow-up patch from `31e432d51a21451ef17ce040e78a41a07527afbf` on
`be24496f5d9b0e989d62269e8d2c0ceba8cd1de4`. The whole-file `coordinator.rs` hash
changes across the rebase; the regression file and all five original proof
files remain byte-identical. The original host positive/mutant/restored
controls, including the initial cache-permission failure before test execution,
remain historical evidence in
`lampOS/docs/verification/requested-audio-voice-state-20261022/`.

The [fresh Mac gate](mac-gate.json) passed on the current head:

| Stage | Passed | Failed | Ignored | Filtered out | Command elapsed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Formatting | — | — | — | — | 4.977 s |
| Strict Voice Clippy | — | — | — | — | 26.959 s |
| Voice tests | 3498 | 0 | 14 | 0 | 440.351 s |
| Skills consumer | 1 | 0 | 0 | 270 | 22.958 s |
| Ears consumer | 1 | 0 | 0 | 66 | 0.418 s |

Every command exited 0 without timeout. Commands ran from `lampOS/`, in order:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --locked --offline -p lamp-voice --all-targets -- -D warnings
cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-voice -- --test-threads=1
cargo +1.96.0 test --locked --offline -p lamp-skills --test it skills::every_skill_folder_passes_its_own_recorded_tests -- --exact --test-threads=1
cargo +1.96.0 test --locked --offline -p lamp-ears --lib classes::tests::a_window_the_lamp_spoke_into_is_its_own_and_the_next_is_unknown -- --exact --test-threads=1
```

All **336 scoped source-file hashes** match before and after. The new lifecycle
regression has an explicit passing result in the retained Voice log. Raw-log
hashes and every summary were checked against the receipt. The gate's `base`
field identifies the tested follow-up head, not its parent.

[intermediate-mac-gate.json](intermediate-mac-gate.json) separately preserves the
successful **unpublished** `31e432d51a21451ef17ce040e78a41a07527afbf` intermediate:
3,492 Voice passes, 0 failures, 14 ignored, 0 filtered; Skills 1/0/0/270 and
Ears 1/0/0/66. Its formatting, strict Clippy and tests passed, and its 336 scoped
hashes were unchanged. These results are historical and do not substitute for
the current gate or native checks.

This fixture proves helper/bus state publication across recording lifecycle
boundaries. It does not exercise the full coordinator service loop, a physical
lamp, acoustic output, motion or latency, and is not a full-workspace gate.
Command durations include compilation. [SHA256SUMS](SHA256SUMS) covers this page
and the four byte-exact input records. Preparing this folder did not run tests,
builds, Git operations, network requests or device actions.
