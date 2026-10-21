# Requested local audio: host evidence, 2026-10-20

This record qualifies the source in this commit for host validation. The
integration base is `dfc5f73c00dbf10a65b668156914d44a19fb4575`, including
landed Bedtime/caps #2157. Automatic ARM64 and x86 CI evidence is recorded
on [PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757) after its
run completes; this host record does not claim those runs passed.

The [source manifest](source-files.json) contains all 2,114 non-documentation
inputs. Its SHA-256, computed over the sorted JSON mapping, is
`8beb566ec9b131ad6c6d12da1229ac60390dcbd55cbb60ccf2c773b142025449`.
All hashes matched both the checkout and its source-only copy after the
entire gate. The [host record](host.json) includes the 47 changed non-doc
input hashes, exact commands, exit codes, elapsed times, result lines and
raw log hashes. It contains no scratch paths or generated audio.

The macOS host used Rust 1.96.0, fresh build directories, no incremental
compilation, two Cargo jobs, and dev/test debug info disabled. All selected
crates ran completely with one test thread and no test-name filters.
The [changed-name scan and dependency closure](consumers.json) selects 29
crates. The complete Update crate ran in the checkout because its inherited
image tests read parent rootfs fixtures; the other tests and the workspace
build ran in the source-only copy.

| Gate | Result | Elapsed |
| --- | --- | --- |
| `cargo fmt --all --check` | Passed | 4.66 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 138.77 s |
| Complete non-Update consumer tests | 7,749 passed, 0 failed, 31 ignored, none filtered | 1,247.48 s |
| Complete Update crate tests | 254 passed, 0 failed, 1 ignored, none filtered | 192.76 s |
| `cargo build --workspace` | Passed | 30.98 s |

The full-suite total is **8,003 passed, 0 failed, 32 ignored, none filtered**.
Separate focused repeats passed 12 current-provider integration tests,
49 audio tests, 24 Skills tests and the single prepared-handoff test; they
are not added to that total. The focused test-name filters and their excluded
counts are explicit in the host record.

The prepared host handoff measured **199 microseconds first use and 194
microseconds repeated**, against the 250 ms target. The boundary is
confirmation retirement to authored speaker IPC acceptance. This does not
measure acoustic onset or disk/network preparation latency.

The four renderer example tests passed separately. Host-debug rendering,
packing and installing into a disposable directory all exited zero. The
48-second output has 1,152,000 mono 24 kHz signed 16-bit frames, 2,304,044
bytes and peak absolute sample 900. Its SHA-256 is
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`.
The package's member list and SHA-256 are in the host record. Every installed
file matched its staged hash. No WAV, package or build binary is committed.

[Earlier failed preflights](preflights.json) remain part of the evidence.
Current Reports fields first required fixture adaptation. A recording-gap
negative then exposed a production completion bug: recording and provider
speech gaps are independent counters. Both must now be zero. Review also
found that cancellation could win the ticket's atomic transition after the
last playback check; completion now requires winning that transition.
Deterministic controls cover cancellation, pre-start completion, success and
duplicate completion. Current integration regressions preserve Policy and
held consent, search(found), warm names, and delayed Written task news.

No device, microphone, provider or audible playback was used. This does not
qualify musical quality, acoustic latency, quiet-hours parity, motion or
optical behavior. Requested audio remains off unless explicitly enabled in
Voice, Skills and Actions; no product or lamp unit enables it.
