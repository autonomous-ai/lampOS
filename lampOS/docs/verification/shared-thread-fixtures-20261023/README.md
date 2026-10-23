# Shared conversation fixtures, 23 October 2026

Second's additional-writer investigation for #1569 row 79, based on public
`ae4ab0022a5716b8b393956d5e26fd1480aac350`. This establishes one reproducible
interference path. It does not identify the cause of the historical failure,
whose panic was not captured.

## Ordered control

The original `the_lamps_own_answers_to_talk_keep_no_conversation_going`
fixture already holds `one_thread()`. At request 3 of its first quiet/off
case, after it installs the conversation and before `heed()` reads it, the
control starts the actual
`the_sentence_after_count_only_never_reaches_the_live_thread` fixture on
another thread. Both original fixtures keep every assertion and bound.

Without a guard in the competing fixture, a channel acknowledges its real
`mem::take(THREAD)` and holds it before restoration. The original row 79
assertion fails: `[true, true, false, false, false]` instead of `[true; 5]`.
The writer is released and joined before that assertion. Result: **0 passed,
1 failed**, exit 101. See `unguarded_writer.patch`, `.txt` and `.json`.

With only the existing guard added at the competing fixture's entry, its
guard attempt is acknowledged while row 79 still owns the lock. Row 79
completes all its original cases before the guard drops and the writer
runs. The writer is joined before the control's channels drop. Result:
**1 passed, 0 failed**, exit 0. See `guarded_writer.patch`, `.txt` and `.json`.
No sleeps or increased product/fixture timeouts were used. Channel deadlines
only bound a broken control. All 6,519 tracked inputs stayed unchanged during
each command; the exact source commit, deliberate patch and changed source
hashes are retained. Both instrumented files were restored byte for byte.

The initial control attempt is retained in `initial-control/`. Its unguarded
run produced the same target failure. Its guarded run completed row 79's
cases but failed in control teardown: the event receiver dropped before the
joined writer sent its reset event. Declaring the channels before the joined
writer corrected that control-only lifetime. Both cases were repeated after
the correction; that initial failure is not a product gate or waived failure.

## Scope of the correction

Five outer fixtures lacked the existing guard:

| Fixture | Shared mutation |
| --- | --- |
| Live sign-in transcript | Clears and writes the conversation; take the conversation guard before its existing agent-task lock. |
| Live CountOnly transcript | Takes, writes and restores the conversation. |
| Ten interrupted questions | Clears the conversation before driving the scripted engine. |
| Photo yes/no answer | Clears the conversation for each scripted scenario. |
| Room light-rule forget | Successful `rule_for` reaches `forget_light`, which clears the conversation. |

The candidate adds five guard acquisitions before fixture work begins and
keeps them through cleanup. Assertions, timeouts and production code are
unchanged. The changed test names occur only in `lamp-voice` under `crates/`.
This is not a claim that every indirect conversation writer is serialized.
The separate Music-stop helper guard and its previously published overlap
control belong to Tests' #3023; this patch leaves that helper unchanged.

## Mac gate and current-base carry

The full changed-crate command on the uninstrumented five-line candidate
passed **3,815 / 0 / 14 ignored**, six terminal results with no filtering:
3,534 + 1 + 270 + 4 + 6 + 0 passed. Formatting and strict all-target Voice
Clippy passed. `mac/` retains the full outputs and their hashes; all 6,519
bound files stayed unchanged during the commands.

The committed candidate `8beb6bd53` carried equally to `5ce68c9a3` on
`9b6960e61`. Only two unrelated Cloud test files changed in the base;
Voice, its dependencies, Cargo manifests/lock and workflows stayed identical.
`rebase.json` binds that carry and the three exact tested source hashes.
Current-base formatting and strict Voice Clippy passed (`mac-current/`).
The completed identical-input full test run was retained, not repeated.

## Official native checks and the next current-base carry

PR #3160 on `5ce68c9a3` passed official ARM64 run **39877124120**, attempt
1: **7,042 passed / 0 failed / 31 skipped**, one passing LEAK. All five
changed fixtures and the original own-answers fixture passed. Linux Clippy
run **39877124100**, attempt 1, also passed. Both checked merge `5e197a237`
with base `9b6960e61`; `native-before-latest-rebase.json` retains exact
checkout identities, job IDs, complete summary/fixture lines and log hashes.
The ARM64 workflow selects affected crates; this is not a 10,799-test full
workspace run. No check was rerun by Second for green.

After the checks completed, the one-commit patch carried equally to
`28cc2539a` on current `02c8d3851`. The base delta contains six Markdown
files only. `rebase-current.json` proves the equal range-diff and unchanged
hashes for every task file. Current-head Mac formatting and strict Voice
Clippy passed (`mac-latest/`). The exact full changed-crate test result above
remains applicable to the unchanged executable inputs. New-head official
checks remain required before ready; Linux formatting is recorded below.

No lamp operation, acoustic result or signed release is established here.

## Explicit Linux formatting

`cargo fmt --all --check` passed on an exact Git archive of `28cc2539a`
in the existing cached Linux aarch64 development image, Rust **1.96.0
(ac68faa20 2026-05-25)**, with network disabled. `linux-format/receipt.json`
binds the full source tree, immutable image ID, command and raw output hash;
`format.txt` is the complete output. This ran formatting only, no local
ARM64 build or tests. The PR workflow remains the ARM64 gate.

Two container setup attempts are retained separately: the minimal Rust image
lacked rustfmt and tried an unavailable offline download; the development
image already had `/src`. Neither reached formatting. The successful
command selects the cached toolchain and extracts into a new empty directory.
No source was changed between these setup attempts.

## Reviewed carry on train 23's integration head

Head `28cc2539a` also completed both official jobs on their first attempts:
ARM64 **39879235794 / 120172316078**, **7,042 / 0 / 31 skipped**, one passing
LEAK; Linux Clippy **39879235765 / 120172315743**. Both checked merge
`78bee58f5` against `02c8d3851`; `native-28cc2539a.json` retains identities,
summary lines and raw-log hashes. Tests posted READ-OK for that source at
08:00 +07, with no P1/P2/P3; explicit Linux formatting above also passed.

Because train 22 changed runtime inputs, the full Mac Voice gate was
refreshed on the equal carry `7a9fc09ba`, based on `f477d6c49`: **3,829 passed
/ 0 failed / 14 ignored** (3,548 + 1 + 270 + 4 + 6), formatting and strict
all-target Clippy passed. All **6,528** bound inputs stayed unchanged.
`mac-train22/` keeps complete output, commands, manifest, and candidate patch
against that public base, so this local tested commit is reconstructible.

The reviewed patch then carried equally to published **`96fa27cdf`** on
**`b7700b7c2`**. The intervening changes are an unrelated Controls fixture,
installer shell helpers and Markdown; all workspace production Rust, Voice
tests and Cargo inputs are unchanged. `mac-train22/carry.json` records the
equal range-diff and changed bound paths. Fresh Mac formatting and exact-head
Linux formatting passed; `linux-format-train23/` binds the latter's immutable
image, source tree and output. The PR is ready under the 07:49 no-drafts rule;
new-head official ARM64 and Clippy checks are in progress. No completed
earlier-head check is represented as a result of this new head.
