# Reminder reset/say: original and train native results

Audit of completed public CI, 26 October 2026, 00:14 +07. No test was
rerun for this record. [PR #3493](https://github.com/autonomous-ai/lampOS/pull/3493)
landed through [train #3494](https://github.com/autonomous-ai/lampOS/pull/3494).

| Scope | ARM64, attempt 1 | Linux formatting/Clippy, attempt 1 | ARM64 nextest output |
| --- | --- | --- | --- |
| Original PR | [40213653347](https://github.com/autonomous-ai/lampOS/actions/runs/40213653347), success | [40213653364](https://github.com/autonomous-ai/lampOS/actions/runs/40213653364), success | 7,437 passed (1 leaky), 53 skipped |
| Train | [40213807332](https://github.com/autonomous-ai/lampOS/actions/runs/40213807332), success | [40213807350](https://github.com/autonomous-ai/lampOS/actions/runs/40213807350), success | 7,437 passed (1 leaky), 53 skipped |

Each ARM64 execution has 7,436 literal PASS rows and one LEAK row before
its Summary: 7,437 unique binary/test pairs, every ordinal 1 through 7,437
present exactly once, zero failed and zero selected cases unrun. The selection
contains 67 binaries; 53 tests and 114 binaries are skipped. These are the
PR's affected-crate selections, not a whole-workspace test count.

All 28 `coordinator::tests::reminders::` cases are literal PASS in each run,
including `reset_or_playback_gap_cannot_be_relabeled_whole_reminder_completion`.
The existing `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
case is the one LEAK in each run. It remains a leaky pass; this record does
not assert leak-free execution or assign a new cause.

## Exact source identity

| Role | Full commit |
| --- | --- |
| Original PR head | `b2d24fc0317813b9e5544ed552eb38617ffc8591` |
| Original actual checkout | `7e41f778573a90404036dd7400235707b2bbedf1` |
| Train head | `82567f96b7c9ab17662719f3f82c90a859faf0ba` |
| Train actual checkout | `4dde3604cf7c4959ae0abfc2d6615749531a17d6` |
| Landed commit | `1ee8cf7f09e6c9c6c7756f825a63640967c22634` |

All five commits have complete tree
`b33c85e51a2c840b3e22c866c6f89d9220668eec`. The checkout rows in the two
selected excerpts identify what CI actually tested; public commit metadata
and Git trees establish the tree equality. A run's event head is not
substituted for its tested merge checkout.

## Census method and retained correction

The initial constituent-log parser failed its count assertion because it
did not accept padded ordinals. Its second attempt failed because it also
counted 34 PASS rows echoed after the Summary. Neither parser failure was
a test execution or test failure. These two failed parsing attempts apply
to the original constituent log only.

The final parser scopes execution rows between the single `Starting 7437`
line and its Summary. It checks unique binary/test identities, every ordinal,
the PASS/LEAK split, and all 28 coordinator reminder outcomes. In both logs,
the 34 later PASS rows refer to already-counted cases and are excluded as
echoed report output. No observation was repeated or converted to success.

[census.json](census.json) records both run identities, conclusions, log
SHA-256 values and reconciled counts. [original-selected.txt](original-selected.txt)
and [train-selected.txt](train-selected.txt) preserve the checkout, start,
summary, 28 reminder rows and leak row with their GitHub timestamps. The
repeated job/step prefix alone is removed. Full logs, source inventories
and binaries are not included in this compact record.

The [local gate](../README.md) remains the preserved unchanged-production
0/1 reproduction followed by full serial Voice **4,053/0/21** on
`56cacbddd`. Its unchanged-input carry to `e1c69457a` and fresh fmt/Clippy
are not another full local execution. Combined execution with #3491 is
separate work owned by Reviewer-two. No device, acoustic, physical-latency
or durable-Schedule-Ack acceptance follows from these CI results.
