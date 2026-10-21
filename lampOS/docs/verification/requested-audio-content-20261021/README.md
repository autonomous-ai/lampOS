# Requested audio full-content verification, 21 October

The complete fresh host gate passed **8,685 tests, with zero failures,
44 ignored and none filtered** across all 36 selected consumers. All 15
stages completed successfully. [host.json](host.json) records exact commands,
counts, durations, source inputs, artifact hashes and raw-log SHA-256 values.
Focused repeats and renderer tests are not added to the complete-suite count.
At this host-gate checkpoint, new native Linux ARM64 qualification was pending.
The later result is recorded with its exact head and merge in the
[requested-audio qualification](../../requested-audio.md).

The correction removes metadata-only reuse. Every scan reads and hashes the
current definition and WAV bytes using the existing optimized SHA-256 backend.
Equal timestamps cannot skip permission or audio verification. It retains no
WAV payload cache and adds no dependency, filesystem qualification or clock
assumption. The lowercase digest and length-prefix encoding remain compatible.

The source-only gate freezes 2,180 non-document files on integration base
`99f2ecfcd4ebb3b9279750bee42f219c0c4895ed`. The [source delta](source-delta.json)
reconstructs their complete hash map from the retained prior map, changing only
the two Rust files. Its combined SHA-256 is
`1fd1c57cca9d64ec7c95aba03f213ddcdf8fcb23c409d0549f385a6ad9ac22c4`.
The [consumer map](consumers.json) covers all 36 changed, name-matched and
reverse-dependent crates. Update runs completely in the checkout for its
parent-image fixtures; the other 35 selected crates and workspace build use
the source-only copy with fresh, separate target directories.

| Command group | Observed result |
|---|---|
| Workspace formatting | passed, 4.49 s |
| Strict workspace/all-target Clippy | passed, 135.28 s |
| 35 source-only consumer crates | 8,426 passed, 0 failed, 42 ignored, none filtered; 1,252.54 s |
| Full Update crate in checkout | 259 passed, 0 failed, 2 ignored, none filtered; 199.54 s |
| Workspace build | passed, 26.91 s |
| Renderer tests | 4 passed, 0 failed, none ignored or filtered |
| Render, package and disposable install | all passed; seven installed files match staged hashes |

All 2,180 source hashes still matched both the original checkout and copied
inputs [after completion](source-check.json). The final focused checks passed
separately: four file checks, one scan-cost test, 12 Voice integration tests,
49 Voice audio tests, 30 Skills audio tests and one prepared-handoff test.
[Development checks](focused-checks.json) retain their own earlier counts and
the sandbox-denied service-fixture attempt separately. The new tests force
equal observed timestamps for changed audio and permission bytes, require
full reads on every scan, preserve stale-scan rejection and isolate path
replacement. The read/hash operation is not an atomic filesystem snapshot.

The 48-second example rendered at 24 kHz, mono 16-bit, with 1,152,000 frames.
Its WAV SHA-256 remains
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`;
the package SHA-256 is
`61618d4ae247536efcfaaa1cb5bf4c4a584e720a9958ebb45b2045bb5c484d35`.
The [host record](host.json) retains all seven staged file hashes and confirms
they match the disposable installation. No recording was played and no device
was accessed during these checks.

The [five mutation checks](mutations.json) passed their exact baselines and
then compiled five independently weakened runtime variants. Every variant
failed the intended behavior assertion: accepting changed WAV bytes, accepting
changed permissions, ignoring a replaced listed path, ignoring a replaced
parent, or accepting a stale successful result after inventory revocation.
The content variants retain their read/hash expressions and suppress only
mismatch refusal. The stale-result variant demonstrates erroneous acceptance
and freshness, not reinserted definitions or revived terminal offers. All
2,180 full-gate source hashes match the restored mutation copy, and every
published patch reproduces its tested mutant hash. Test sources are unchanged.

The fresh gate's completed [scan-cost check](scan-timing.json) reads
46,085,516 bytes per pass across sixteen 60-second recordings: **23.550625 ms
first, 22.874458 and 23.183833 ms repeated**, against a 250 ms host target.
The timed boundary is `Scan::unchanged`; fixture creation, initial loading,
snapshot cloning, authority refresh and logging are excluded. Recently
written/read filesystem pages mean these are not cold-disk, native Linux
ARM64, acoustic or physical-lamp results. The focused check passed one test;
its command, source aggregate and raw-log SHA-256 are in the timing record.

The [prepared-handoff check](handoff-timing.json) measured **186 us first use
and 165 us repeated** against a separate 250 ms host target. The boundary is
confirmation retirement, with the recording prepared, to authored speaker IPC
acceptance. It excludes cold preparation and spoken confirmation. The focused
test passed; acoustic onset, physical Stop timing and the complete 48-second
example still need the [planned lamp-4ace run](../../requested-audio-lamp-run.md).
These measurements qualify the recorded host fixtures, not a physical lamp.

The [earlier native failure](earlier-native-failure.json) is retained: current
at that time, head 6f4e5ceea0df9f47ec211e5649dedf075507a7d6 failed the immediate
same-size rewrite regression. Its log did not record actual stamp values.
The earlier zero-read measurements and cache-specific mutations qualify their
recorded historical implementation. The prior unchanged Setup intermittent
failure and diagnostic outcomes remain in the
[previous host record](https://github.com/autonomous-ai/lampOS/blob/d2e171f9ac5af88955e631812fa0038b24fa1af8/lampOS/docs/verification/requested-audio-host-20261021-current/README.md).

`LAMP_REQUESTED_AUDIO` remains off by default and must be enabled explicitly
in Voice, Skills and Actions. No service unit or install step enables it.
With output caps on, a rejected volume argument that performs no mixer I/O
preserves an already-confirmed cap; actual mixer reads/writes still update
the observation even with requested audio off. These behavior boundaries are
unchanged by the content-verification correction.
