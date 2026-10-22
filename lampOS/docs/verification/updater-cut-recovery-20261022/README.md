# Updater cut-recovery focused host evidence

[Issue #2810](https://github.com/autonomous-ai/lampOS/issues/2810), based on
[`f71a62f55961`](https://github.com/autonomous-ai/lampOS/commit/f71a62f559619c996d6a85f253ed88e367549722)
plus the test and documentation hashes in [baseline.json](baseline.json).
**Both scoped Update gates passed, including the rebase onto `9c2ede339f8d9e55de809e2bf4c8b1606d4e165c`. ARM64 CI and issue acceptance
remain pending; this receipt does not claim completion.**

| Run | Result | Command wall time | Test time |
| --- | --- | ---: | ---: |
| Initial focused baseline | 6 passed, 0 failed, 0 ignored; 99 filtered | 57.090 s | 32.05 s |
| 12 deliberate controls | Each compiled, then 0 passed / 1 intended failure; 104 filtered | 131.408 s total | 50.37 s total |
| Restored focused baseline | 6 passed, 0 failed, 0 ignored; 99 filtered | 36.439 s | 31.14 s |

All commands used Rust `+1.96.0`, `--locked --offline`, and one test thread.
Baseline/restored exit codes were 0; each control exited 101 without a
timeout. Exact commands and timestamps (+07) are retained in the receipts.
The 33 recorded source hashes match before/after both runs and the restored
files inspected for this export. The six passes after restoration repeat
the original six fixtures; they are not twelve distinct tests.

The five steps cover candidate staging/sync, pending saved before link
replacement, current moved, confirmation cleared, and interrupted put-back.
Distinct old/new scripts travel through the normal signed build path and
are executed at boundaries; the fixed fallback's bytes and mode stay the
same. Put-back is blocked both before replacing the link and after restoring
old but before recording refusal. An additional subcase reconstructs the
post-refusal/pre-cleanup prefix by completing real put-back, then restoring
only the exact captured pending bytes. It checks cleanup and no duplicate
refusal; it is **not an actual execution cut at that point**. The sixth test
keeps parts updates working with self-update opt-in absent/off.

The following assertion locations are in
[`self_cuts.rs`](../../../crates/update/tests/round/self_cuts.rs), whose exact
hash is recorded in baseline.json. [controls.json](controls.json) includes
the raw-log line numbers, hashes, commands and observed failure details.

| Control | Source line | Observed behavioral failure | Test seconds |
| --- | ---: | --- | ---: |
| [R1](R1-prune-old-before-handover.patch) | 29 | current file absent | 4.11 |
| [R2](R2-unlink-before-atomic-replacement.patch) | 29 | current file absent | 3.48 |
| [R3](R3-select-renamed-away-temporary-build.patch) | 29 | current target absent | 3.58 |
| [R4](R4-remove-selected-build-during-confirmation.patch) | 29 | current file absent | 3.78 |
| [R5](R5-delete-failed-build-before-restoring-old.patch) | 29 | current file absent | 3.92 |
| [S1](S1-omit-final-candidate-rename.patch) | 29 | final candidate absent; old runs | 3.74 |
| [S2](S2-omit-pending-record.patch) | 123 | pending absent | 3.81 |
| [S3](S3-omit-current-switch.patch) | 29 | old bytes instead of new | 4.31 |
| [S4](S4-retain-pending-after-confirmation.patch) | 146 | pending not cleared | 4.04 |
| [S5](S5-clear-pending-before-refusal.patch) | 310 | None instead of saved pending | 3.78 |
| [O1](O1-bypass-self-opt-in.patch) | 367 | SelfSwitched instead of Switched(2) | 3.75 |
| [S6](S6-skip-stale-pending-cleanup-after-refusal.patch) | 347 | Nothing instead of PutBack | 8.07 |

R1–R5 fail reading the selected current executable with `NotFound`. They
break the selected entrypoint; surviving old/new build copies and the fixed
fallback are documented per control. They do **not** show every updater copy
removed. S1–S6 test state/order contracts while an updater can remain
runnable; O1 tests opt-in. None is counted as a runtime regression after
restoration. The caught `Cut` sentinel panics in successful baseline logs
are deliberate callback stops. S2 and S4 each introduce one compiler
warning in mutated source (unused `begin` / unused `failed`); baseline and
restored runs have none.

[proposed-manifest.json](proposed-manifest.json) and all 12 patches are
byte-identical preparation artifacts. Its “not run” statuses are historical;
[execution-map.json](execution-map.json) links that unchanged manifest to
observed results. Raw logs remain private and are bound by SHA-256 only;
they are not copied into this folder. [SHA256SUMS](SHA256SUMS) binds every
published file here.

These are logical filesystem-prefix checks, not physical power-loss or
fsync durability proof. They exercise no real systemd timer, daemon exec
handover, device, network, or release publication.

The [initial full Update gate](initial-mac-gate.json) passed on the recorded
source: formatting 4.925 s; strict Update Clippy 17.185 s; all Update tests
**297 passed, 0 failed, 2 ignored, 0 filtered**, 256.688 s command wall time.
Every command exited 0, and all 33 recorded source hashes remained fixed.
Changed-helper searches across crates find only Update test consumers, all
covered by this crate gate. The current-base rebase is qualified separately below.

The [fresh post-rebase gate](current-mac-gate.json), on source head
`c643024726ac9525eafe2a4843e6a0388ffc7911` before this evidence amendment,
passed formatting (4.863 s), strict Update Clippy (16.940 s), and all Update
tests: **297 passed, 0 failed, 2 ignored, 0 filtered**, 281.847 s command
wall time. The [range-diff](range-diff.txt) is equal, and the
[rebase identity](rebase-identity.json) records all 33 unchanged scoped
source hashes. The final evidence amendment changes no tested source.
