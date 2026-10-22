# Requested recording voice/state: native positive and negative evidence

[PR #2879](https://github.com/autonomous-ai/lampOS/pull/2879), head `4e060a224cb3d04bcd4404355aa5fc0bac187705`:
**positive native CI and the required native negative control are qualified;
ready for review.** The reviewer decides merge. No merge or deployment is
recorded here.

The [positive ARM64 run 39718009844](https://github.com/autonomous-ai/lampOS/actions/runs/39718009844)
and [Linux Clippy run 39718009936](https://github.com/autonomous-ai/lampOS/actions/runs/39718009936)
completed successfully. Their metadata, checkout logs and immutable parents
identify tested merge `6b640cbd776198c3b51f75c216b0c9b1c0ee1f4c`: integration
`9c2ede339f8d9e55de809e2bf4c8b1606d4e165c` plus the positive head above.

```text
Summary [ 173.104s] 10186 tests run: 10186 passed (1 leaky), 63 skipped
```

All 10,186 primary identities and ordinals reconcile: 10,185 PASS and one LEAK,
with no failures or timeouts. The 63 profile skips remain skipped; their complete
names are not emitted. The leaky result is `lamp-voice`
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
The exact new regression passed at ARM64 log line 7510, ordinal 6823/10186,
in 0.028 seconds:

```text
lamp-voice coordinator::tests::requested_audio::requested_audio_keeps_voice_state_non_resting_until_final_retirement
```

The test reads eight published `voice/state` boundaries through the unchanged
production mapping helper: Thinking when prepared and first dispatched;
Speaking through acceptance and outstanding or accepted-but-unretired final
playback; Thinking after retirement while owned; Resting after completion.
Its source blob matches the immutable positive tested merge.

The [negative ARM64 run 39718365623](https://github.com/autonomous-ai/lampOS/actions/runs/39718365623) deliberately failed on
`672023248b37d1f1b226b0b1758ee8503807b7d0`, whose **only parent is the positive head**.
The immutable commit changes only `lampOS/crates/voice/src/coordinator/notices.rs`:
[control.patch](control.patch) adds `RequestedAudio` to `unshown()` and formats
the existing alternatives. Applying that exact patch in memory reproduces the
recorded mutant SHA-256 and immutable Git blob.

```text
Summary [ 129.123s] 6820/10180 tests run: 6819 passed, 1 failed, 63 skipped
```

Its sole failure is the same regression, at native log line 8212,
ordinal 6817/10180. At `requested_audio.rs:710:9`, the
`recording prepared` assertion observed **Resting**, expected **Thinking**.
The 6,820 unique primary rows reconcile exactly. **3,360 selected tests were not
run** after failure; 63 profile skips are separate. The 23 PASS rows and one
repeated FAIL after the summary are diagnostic replay and excluded from totals.

The control's test step failed with process exit 100. Doc tests, custom-harness
and relay checks, artifact export/upload, and publication were skipped.
The artifact API reports zero control artifacts. The expected assertion failure
qualifies the control; the interrupted run is not relabeled a passing full gate.

**The native runs did not use identical whole integration trees.** Positive
PR CI tested the later `9c2ede3` + `4e060a2` merge; the negative control is a
direct child of `4e060a2`. Their selected totals therefore remain separately
reported (10,186 and 10,180), without treating them as an identical-tree A/B run.

[mac-control.json](mac-control.json) and [control-identity.json](control-identity.json)
are byte-exact historical Mac receipts. Formatting and strict Voice Clippy
passed; the focused mutant compiled and failed as intended: **0 passed / 1
failed / 0 ignored / 3,235 filtered out**. The 336 scoped hashes stayed unchanged
with the mutant applied, and only `notices.rs` differs from the positive Mac
source map. The copied identity's old `native-pending` label is preserved as
history; the terminal native record in [ci.json](ci.json) is authoritative here.
The separate [positive Mac gate](https://github.com/autonomous-ai/lampOS/blob/19392a533aad47002e009926fc003d9083ad6b6c/lampOS/docs/verification/requested-audio-voice-state-rebase-2d1d3-20261022/README.md) records
3,498 Voice passes, 0 failures, 14 ignored and 0 filtered, plus Skills 1/0/0/270
and Ears 1/0/0/66.

Positive native doc tests: **4 passed / 0 failed / 0 ignored**; custom-harness:
**12 passed / 0 failed**; relay: **passed**. Strict Clippy used
`cargo clippy --locked --workspace --all-targets -- -D warnings` and emitted zero Rust warnings.
Each ARM64 run has eight example-output collision warnings; the control also
retains its unrun-test warning. Certificate/Node notices and every terminal job
and step state are recorded in `ci.json`.

The positive artifact API reports one unexpired head-matching artifact; its ID,
size and reported digest are metadata only. No artifact payload was downloaded
or independently verified.

Full retained log SHA-256:

- Positive ARM64: `09307501e92582a4281380a7b5baaa1bdc765df187f4ed5f48539270f867bd1b`
- Positive Clippy: `0254d5c0a89c338bd3ca96091f24949cc1d2ab3a810a5e13c5f8a7e0d96ea839`
- Negative ARM64: `dd34ce161e811f419e2568b9233d99c313063b3ba2fea42455c22a419273919a`

[SHA256SUMS](SHA256SUMS) covers this page, the combined receipt, exact patch and
two copied Mac records. The helper/bus fixture does not establish full
coordinator-loop, physical-lamp, acoustic, motion or latency behavior. Test times
are not interaction-latency measurements. Preparing this proof ran no tests,
builds, Git operations, network requests or device actions.
