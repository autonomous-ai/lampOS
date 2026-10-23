# Head pins and installed readers, 23 October 2026

This is host evidence, on Darwin ARM64 with Rust 1.96.0. No lamp was accessed,
no physical touch was performed, and no physical latency or flash-durability
claim follows from these tests.

## Final review-correction host gate

On latest integration `810e05109`, the final source passes **371 / 0 / 4 ignored**,
including **151 round tests** and **28 head-touch cases**. Both exact Third
regressions are retained unchanged and pass. Mac `cargo fmt --all --check`,
source-only Linux formatting and strict Update Clippy all pass.
[Full test output](final-update-tests.txt), [Clippy](final-clippy.txt),
[Linux formatting](final-linux-fmt-summary.json), [source blobs](final-host-source.json).
The following evidence-only commit amendment does not change these Rust blobs.
Official ARM64 and Linux Clippy must rerun on the corrected pushed head.

The final three cases reject invocation-variable expansion in semantic pins,
bus and mute paths, even when all unit strings look equal, and malformed
Environment quoting. The ordinary session output's invocation variable remains
supported by the existing product fixtures. No extra live touch-path work is added.

## Original regression and first submitted gate

Third's two controls from public receipt `f99783904` were brought onto
`3e75cc3cb`, with old files frozen byte-for-byte from `efc8db9cb`. The proposed
#2948 pins are formed by removing only the legacy_head line, so they stay
independent of changes to product defaults. Before any product edit they gave
**1 passed, 1 failed**: settings-on reached Switched(2) with no head reader.
[Raw original failure](before.txt).

The first submitted head, `c062243b6`, had this source gate on `0f6f728b2`
plus its patch (the later base changes were documentation only):

- `cargo +1.96.0 fmt --all --check`: pass on Mac and in cached Linux image
  `sha256:7dc3bb4b3b88bb00e16e37eeb5c98a74f9d926f956584eb8c009c9ca86023cd2`.
  Linux used a source-only stream, no network and no compilation. Its first
  bind-mount attempt failed because the VM could not mount the temporary
  worktree; the source stream succeeded (tar reported ignored macOS metadata).
- `cargo +1.96.0 clippy -q --locked --offline -p lamp-update --all-targets --message-format=short -- -D warnings`:
  pass. Two initial iterator diagnostics were corrected with next_back.
- `cargo +1.96.0 test -q --locked --offline -p lamp-update`:
  **357 passed, 0 failed, 4 ignored**, including **137 round tests** and the
  fourteen head-touch cases. [Complete output](update-tests.txt). The two
  Linux process harnesses are among the Mac ignores; official ARM64 CI is
  required separately.
- Direct dependents were also attempted with `cargo test -q --locked --offline
  -p lamp-update -p lamp-setup -p lamp-cloud -p lamp-skills -p lamp-keeper`:
  Cloud **196/0/2 ignored**, Keeper **57/0/1 ignored**, Setup **162/0/1 ignored**.
  Skills library **156/1/0**: `daily::tests::pre_rename_failures_preserve_bytes_and_poison_the_owner_without_consuming_a_day`
  failed reopening its unchanged directory lock after dropping the owner.
  The command stopped there, so it supplies no Skills integration result.
  [Original failure](dependent-tests.txt). The Skills source is unchanged;
  a clean integration comparison is tracked in the PR rather than silently
  retrying or modifying it here.

The earlier nine-case run passed 9/0. An intermediate full updater run passed
353/0/4. Its first rollback fixture incorrectly made both old and new mocked
Expression processes crash; that fixture was corrected to make only the signed
candidate binary crash, retaining the successful-old-recovery assertion. These
intermediate results are not substituted for the submitted-head counts above.

## What the tests establish

`tests/round/head_touch.rs` exercises the real round and apply helper with
signed mock builds, filesystem state, mocked systemd and the existing clock:

- original settings-off and settings-on transition controls;
- new Voice, absolute mute record and Expression enabled together, even when
  signed pins still request legacy delivery;
- both mixed-reader combinations, missing/invalid mute record, unrecognized
  reader and relevant local overrides refused before writes;
- settings-off incompatible transition refused; custom/calibrated pins kept
  or the incompatible transition refused;
- exact old settings/units restored after a candidate failure;
- an older apply helper's incompatible output restored before any reader restart;
- a cut after pins apply with unchanged binaries/units still restarts Controls
  after restoring its old pins;
- repeated projected pins retain actual-byte hashes without another Controls
  restart; a partial manifest omitting Controls is refused.

The helper simulation is the old algorithm's output/receipt behavior, **not an
execution of a historical helper binary**. A fixed helper installed before this
change needs `lamp-update install` to gain projection; updater self-switch does
not replace it. The new round safely rolls back a mismatched old-helper result.
The broader inherited IMAGE-on/settings-off combination is not qualified by the
ordinary settings-off control. No consent or private-person record is changed.


## Review corrections after c062243b6

Third's HOLD identified different effective bus roots and a fixed product pins
file mistaken for Controls' active file. The [independent controls](third-regressions.rs.txt)
are copied unchanged from `e73ced777`; Third recorded **0 passed / 2 failed** on
the held head. The expanded local [before run](review-before.txt), with production
unchanged, recorded **16 passed / 4 failed**, including all original fourteen
controls passing. An intermediate [after run](review-after.txt) passed **23 / 0**;
that is not the final expanded gate.

The correction follows Controls' effective `--pins` path. Only active,
build-owned product pins are projected; active custom/build-local pins are
checked without rewriting, and unused product pins follow ordinary settings
handling. It also compares the effective Controls, Expression and enabled
Voice head/privacy bus roots, including flags, environment defaults and unknown
relevant overrides. Legacy gestures still require Controls and Expression to
share their bus. The exact reviewer controls, absent/stale unused files,
environment-only roots, drop-ins and matching/default roots are retained.

The separate [clean-base Skills run](base-skills.txt) at `0f6f728b2` passed
**512 / 0 / 1 ignored**. The earlier changed-head dependent run's directory-lock
failure did not reproduce there, so neither a clean dependent gate nor an
inherited cause is claimed. Tests identified the failure boundary as immediate
reopen after dropping the journal, before the next day's claim; no product retry,
delay or weakened assertion was added.


The intermediate 25-case source (before the argument-expansion/quote correction)
then passed the full Update crate: **368 / 0 / 4 ignored**, including **148 round
tests**, on base `2615c87b6`. [Raw intermediate gate](review-expanded-25.txt).
The final source adds three parser refusal cases and is rebased separately;
this earlier run is not presented as its gate.


The original held head `c062243b6` also passed official ARM64 run
[39952201001](https://github.com/autonomous-ai/lampOS/actions/runs/39952201001):
**11,025 / 0 / 68 skipped**, one passing leaky case; doctests **4 / 0**, Linux
process harnesses **12 / 0**. [Bound summary](original-native-summary.json).
Linux Clippy [39952200929](https://github.com/autonomous-ai/lampOS/actions/runs/39952200929)
passed too. Those gates did not contain the review regressions and do not clear
the HOLD or qualify the corrected head.
