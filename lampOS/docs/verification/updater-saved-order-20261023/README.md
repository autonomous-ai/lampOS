# Saved updater rollback order, 23 October 2026

Follow-up to #3173: a valid switch must not lose its way back because an
unrelated installed unit becomes unreadable or changes dependencies after
planning. The new journal records the validated old startup order before
any switching write. It includes inactive changed units and dependency
paths through unchanged units. Recovery checks the full journal before
excluding an already-restored prefix, then restarts only active members
that still need restoration. Unreadable installed inventory also keeps
build folders and their `made` records during cleanup.

## Tests before production changes

The exact test-first patch has SHA-256
`61f620b1643dc4845122d20c173903b7903e2adee5259150aa11378870d2924a`.
`baseline.json` and its complete output record **8 passed / 6 failed** on
unchanged production at local head `3bcaf5eb6`. The Update tree there is
identical to public integration base `b3a1ece0b`; `final-binding.json` records
that comparison. This is a source comparison, not a repeated baseline run.
The failing cases reach behavioral assertions, not compilation/setup errors.
The legacy-journal case and the seven existing cases already pass.

The unchanged test patch passes **14 / 0** on the candidate (`positive.*`).
It covers unreadable installed text, drift through an unchanged transitive
dependency, a cut at reload after the full order was durably written,
interrupted put-back with an inactive member, 11 malformed saved-order
forms, field-absent legacy recovery, and a no-unit journal with an unreadable
inventory that must preserve a live build during cleanup.

Disabling only the saved-order path restores the unreadable-inventory
failure, **0 passed / 1 failed** (`ignore-saved-order.*`). The full working
patch with this deliberate control is retained. `control-restoration.json`
proves the source was restored byte for byte before the full crate gate.

## Required Mac gate

Darwin arm64, Rust 1.96.0, on `b3a1ece0b` plus this change:

```
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -q --locked --offline -p lamp-update --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test -q --locked --offline -p lamp-update
```

All three commands return zero. Tests: **332 passed / 0 failed / 4 ignored**
(119 + 4 + 78 + 8 + 123). The ignored cases are the manual fixture writer,
a live GitHub download, and two Linux tracing cases. The fixture writer was
run separately, **1 / 0**, to generate the new 23 October JSON pair; the
21 October pair is untouched and both dates are read in the full gate.

Each command's JSON names its exact arguments, elapsed time, exit status,
raw-output SHA-256 and source count. All **6,654** bound regular files stayed
unchanged during the gate. `.txt` files are complete, unedited command
output. The early baseline and positive runs retain their own bindings;
they are not relabelled as the full final run. Only prose was clarified
after that gate; `final-binding.json` checks the tested Update/Cargo inputs.

## Compatibility and limits

The new journal field is optional. New code reads old journals and retains
the old graph-reconstruction path without rewriting their unknown fields.
Old readers ignore the added field. A present malformed order retains the
journal and restoration prefix; malformed JSON syntax keeps the previous
set-aside behavior. Old journals can still be blocked by unreadable or cyclic
live inventory. A saved order does not repair missing saved files, failed
restoration writes, systemd failures or service health.

Cross-crate searches find no outside consumer of `records::Journal`,
`rollback_order`, the private `plan` or `restore_order`. Cloud and Skills use
other unchanged Update APIs. There is no new switch, message, topic or unit
line. Wrong-person, privacy, forget and Voice quiet-boundary logic is unchanged.
Source self-review found no P1/P2; the wording now states that the saved
restart order survives graph-read failures, not that every restoration succeeds.
The mixed-version window is documented in both directions with the default
3600-second quiet bound and managed-base-file-only dependency coverage.

These are fake-systemd/file fixtures and existing process tests on a Mac.
The caught reload panic models a persisted cut point, not a physical unplug.
No lamp, real systemd restart, flash durability, real release key or acoustic
latency was tested. The PR's official ARM64 and Linux Clippy gates are separate.

## Published head

PR #3186 is one commit `3a332edd3` on latest integration `b3a1ece0b` at
publication. `published.patch` reconstructs the commit from that public
base, and `final-binding.json` confirms every tested Update/Cargo input
matches the published source. Exact-commit Linux formatting also passed
in the existing Rust 1.96.0 aarch64 container with network disabled;
`linux-format/` records the source tree, immutable image ID and raw output.
No local ARM64 compilation or test gate ran. Official PR checks remain
separate and pending at this checkpoint.

## Carry onto train 28

The integration branch advanced during PR creation. The one-commit carry
is `c02328dd7` on `70cbefcee`. `git range-diff` reports `=` and all eight
authored files are byte-identical (`train28-carry.json`). The new base changes
Voice, a test dependency, and an embedded product-unit comment, so the
required Mac Update gate was refreshed rather than treating the earlier
execution as a current-base run.

`mac-train28/`: fmt and strict all-target Clippy pass; tests again report
**332 passed / 0 failed / 4 ignored** (119 + 4 + 78 + 8 + 123), with all
6,655 bound files unchanged. `linux-format-train28/` records a successful
formatting check from exact committed source `c02328dd7`, networking off.
The latest integration ancestry was checked immediately before the own-branch
push. Official PR ARM64 and Linux Clippy run separately on the new head.

The original `3a332edd3` ARM64 run 39890406276 and Clippy run 39890406136
both ended **cancelled** when the current-base carry was pushed. Their
terminal states are retained in `superseded-native-state.json`; neither is
counted as a successful native gate or substituted for the current-head run.

## Final official PR gates

The exact current head `c02328dd7` passed both official checks, first attempt:

| Check | Run | Job | Result |
| --- | --- | --- | --- |
| Linux ARM64 build/test | 39891358014 | 120211963627 | **7,129 passed / 0 failed / 31 skipped**, one passing leaky case |
| Linux Clippy, all crates and targets | 39891358186 | 120211950470 | Success |

Both checked the PR merged with `70cbefcee`. `native/receipt.json` binds the
actual checkout, logs and all 26 ordering/parser/quiet/record/self-switch
results. It includes the actual CLI exec case (**0.135 s**) and persisted
self-switch SIGKILL harness (**3.464 s**). Full raw logs are kept unchanged.
Release test compilation took 11 min 11 s; nextest itself took 138.541 s.
The long step was compilation, not a missing test result. Subsequent doc and
own-harness checks passed; signing/publication was skipped on this PR.

Artifact **12475646668**, `lampos-arm64-c02328dd77486b3b7c381ede9f0d4b5365cf3d4f`,
is available from the ARM64 run. `native/artifact.json` records its API
identity, archive SHA-256 and expiry. This metadata is not selection or
verification of individual binaries for a lamp; First still owns the actual
candidate/fallback pair and attended procedure.

Nine and Reviewer-lead both posted READ-OK for `c02328dd7`, with no blocking
finding. Integration advanced to `e5cd73aca` during CI. `integration-carry.json`
records a clean computed merge with all eight authored files identical and
the same builder rules. This comparison did not change the reviewed branch
and is not a new execution: First must gate the later train composition.
No lamp, physical unplug, flash durability or real-key release is claimed.

## Train 31 and the landed source

#3186 landed through [train #3192](https://github.com/autonomous-ai/lampOS/pull/3192)
at `498aca8a2` (10:34 +07). Its updater commit is `e3d9324df`; range-diff
from reviewed `c02328dd7` is `=`, all eight authored files match, and the
complete landed tree matches tested train `f42887330`.

The actual train ARM64 gate, run **39893963040**, reports **10,927 passed /
0 failed / 68 skipped**, one passing leaky case; Linux Clippy run
**39893962998** passed. [Train receipts](train31/README.md) contain both
complete raw logs, checkout and landing bindings, 35 specific result lines,
and the artifact identity. The CLI exec case is **0.135 s** and the SIGKILL
harness **3.559 s** on this composition. #3130's reviewed carry also landed;
its source comparison and scoped read are retained beside the native proof.

This completes the software carry and required gates. #2810's actual
candidate/fallback, observer and attended 52e6 unplug remain with First,
Fifth and the attending person. #1812's protected publisher/real-key run
follows attended acceptance. No lamp or real release was performed here.
