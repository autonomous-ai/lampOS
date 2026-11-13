# Keep-at-once: value-letter A coverage

Fourth claimed stalled #5919 in comment 6752770301 after checking the PR,
Queue and open/merged replacements. The original authored commit
`2f906bb0bafb56d84542b44ab7a466f3a7e8d4d4` already uses current base
`a2260fd1b85ed901ced9d1a083c893874b1d6bd3` and is retained unchanged.
Friday's branch is untouched. This receipt accompanies Fourth's draft carry;
current-head Linux CI and the required reads remain separate gates.

## Boundary and repair

The fast acknowledgment runs after the remember calls return and before the
Engine decides whether to request another model round. Coverage must not
silently omit a requested value. The added local comparison targets under
1 ms on this host; it adds no I/O, polling, timeout, task or new stored state.

If the normalized requested payload contains standalone `a`, one kept text
must contain the same ordered normalized content with every `a` retained.
The existing membership check still applies. This prevents an unrelated
article, including one in a different kept call, from standing in for a
requested letter A. Unchanged A and an ordinary article paraphrase remain
fast. Reordered, split or otherwise uncertain a-bearing paraphrases can take
an extra model round. That model/network delay is not measured here. This is
conservative lexical matching, not semantic equivalence. The existing `i`
exclusion remains; the switch stays off. No memory-store, model or lamp
acceptance is inferred.

## Fail-first Engine evidence

Actual Engine with synthetic STT, chat, tool results and audio. The unchanged
matcher plus three added Engine tests ran eleven cases: ten passed, one
failed, zero ignored, 0.09 s body, exit 101. Both changed-value variants
incorrectly gave the fixed acknowledgment and asked one chat round:

- Requested blood type A; kept `Blood type is B.`
- Requested blood type A; kept `A person has blood type B.`

Switch-off controls asked two rounds. Unchanged A-to-A and `I have a cat`
to `Has a cat` passed as fast acknowledgments. This proves the fast-answer
and round-suppression consequence with scripted successful tool results;
it does not demonstrate a real memory-store write or external model result.

The initial repository-root invocation with `--manifest-path` stopped in the
camera build script because workspace Cargo configuration was not loaded.
No tests ran there. The fail-first result above came from `lampOS/` with the
required configuration. Both logs remain outside the repository.

## Standalone coverage and latency

Rust 1.96.0, optimized standalone binaries. The entire production prefix of
keep_at_once.rs is copied byte-for-byte, together with actual lower_words,
about_fold/search_text, plural_off and SEARCH_COMMON helpers. Real serde_json
and unicode_normalization libraries are linked from the retained target.
Only the unrelated asks_to_keep dependency is stubbed; it is not invoked
by covers. This is a covers probe, not a package or Engine test suite.

Sixteen acceptance cases: original source has seven mismatches, corrected
source has zero. Some of these encode the newly conservative split/reorder
boundary rather than separate original defects. The same input set supplies
1,000 timed calls; the first call is timed before other probe invocations.
The measured interval includes the black-box and clock-call overhead.

| Source | First | Median | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Original | 187.597 us | 110.836 us | 158.969 us | 266.179 us |
| Final corrected | 183.205 us | 110.901 us | 135.424 us | 538.438 us |

Both meet the 1 ms host target in this sample. No process startup, full turn,
slow-service or physical latency claim. Probe sources, hashes, commands and
logs are retained under `/tmp/fourth-keep-letter-probe/`. The initial corrected
sample also passed before the final module-comment edit (first 189.822 us,
maximum 222.409 us); the table binds the final production prefix.

- Original production-prefix SHA-256: `76efdf438ac6c644beb1105c163e3486bd8c49e87aea935739e331db34dbadf3`
- Final production-prefix SHA-256: `c179bbee8ab52ca6d1416cf6b22bd5f3db6fa066bcb5c9469b45b8a7cbddfba7`
- Actual copied helpers SHA-256, unchanged: `03712f0d6227fde1f38961324c8226cd87ec2d0600e2037b6b39c86c063f5718`

## Native gates and remaining checks

Whole-workspace formatting, strict workspace all-target Clippy, diff and
docs checks pass (192 documents, 1,898 links, zero broken). The focused
keep_at_once filter passes twenty native tests, zero failed or ignored,
0.14 s body. The full Voice library run failed: 5,463 passed, two failed,
16 ignored, 689.30 s test body (748.755 s command), exit 101. Both failures
observed two HTTP requests where one was expected:

- `second_provider::tests::chat_second_try::a_head_in_time_and_a_slow_first_word_send_no_second_try`
- `second_provider::tests::chat_second_try::a_head_and_nothing_after_it_sends_no_second_try`

Those tests and their HTTP fixture are unchanged from the base. Their
scripted turns contain no tools, so they do not exercise keep-at-once
coverage. The cause of the extra requests is unproven. This remains a
failed full-suite result; isolated diagnostics cannot turn it green.
Remaining package targets and workspace build are checked separately
without repeating the full suite. Current-head Linux CI is pending. English
docs cover the actual lexical boundary; these three lampOS pages have no
Vietnamese counterparts. Thirteen's current-head read and the original lamp
check remain separate. No switch-on or deployment is included.

Native gate commands from `lampOS/`, Rust 1.96.0, DEBUG0 for dev/test,
incremental off, two build jobs, short owned TMPDIR and the sole retained
Cargo target. Formatting completed before the guarded controller; the
remaining stages execute sequentially and stop on failure.

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -q -p lamp-voice --lib keep_at_once -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2
cargo +1.96.0 build --offline -q --workspace
```

The first three gates passed; full Voice failed and the controller stopped
before workspace build. A separate controller runs the two exact failing
tests, then the previously unrun integration, binary/example and doc targets,
and workspace build. All six separate stages passed, with all five guarded
hashes unchanged:

| Separate stage | Result | Command wall time |
| --- | --- | ---: |
| Exact slow-first-word diagnostic | 1 passed | 1.958 s |
| Exact head-only diagnostic | 1 passed | 3.364 s |
| Integration targets | 279 passed, 6 ignored | 30.064 s |
| Binaries and examples | 11 passed | 9.266 s |
| Voice doctests | 0 cases, command passed | 3.588 s |
| Workspace build | Passed | 122.722 s |

These runs do not replace the failed full-suite result. Their commands were:

```sh
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::chat_second_try::a_head_in_time_and_a_slow_first_word_send_no_second_try -- --exact --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::chat_second_try::a_head_and_nothing_after_it_sends_no_second_try -- --exact --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --test '*' -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --bins --examples -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --doc
cargo +1.96.0 build --offline -q --workspace
```

The controller hashes both changed Rust
files and all three behavior docs before and after each stage. The
verification README is outside that source guard so results can be recorded.

## Added-negation follow-up, 10 November

Thirteen's HOLD 6758394575 found the converse of the earlier lost-negation
case: "I like tea" kept as "Doesn't like tea" passed the one-way subset
check. On local carry 07b3b689a (base 90d9948c4), an expanded standalone
probe reproduced five added-negation failures in 22 cases, exit 1. The
repair compares the presence of the marker emitted by `not_in` on both
sides, after the same normalization. All 22 cases then passed, exit 0.
The probe controller itself exits zero after writing its result; these
exit codes are the tested binaries' codes, recorded in their JSON receipts.

Measured over the same 1,000 coverage calls, first / median / p99 / maximum
elapsed time was 185.498 / 110.612 / 163.656 / 619.432 us before, and
280.102 / 114.107 / 222.789 / 570.515 us after. Both samples meet the 1 ms
coverage-only target. No network, real memory-store or device measurement.
Artifacts are `negation-before` and `negation-after` under the same scratch
probe directory. New unit and actual Engine tests cover added negation
falling through to round 2 and equivalent negation staying fast. Native
results are recorded below; older passes and reads do not qualify this repair.

This remains lexical coverage. Matching the presence of negation does not
prove its scope in multiple clauses, nor repair the separately reported
question-word and other value-token limitations. The product switch stays off.

### Native follow-up and train 767 rebase

On 07b3b689a plus the six-file repair / base 90d9948c4, formatting passed
in 18.618 s, strict workspace/all-target Clippy in 212.207 s, and all 23
focused keep-at-once module/Engine cases passed (zero failed or ignored,
1.47 s body; 190.350 s command). Full Voice failed: 5,477 passed, two failed,
16 ignored, 761.24 s body / 829.868 s command. Both failures were extra
HTTP requests: `a_look_never_gets_a_second_try` counted two versus one, and
`a_round_after_a_tool_never_gets_a_second_try` counted three versus two.
The latter calls check_the_time, not remember. Their test/fixture source is
unchanged by this repair; the origin of the extra requests is unproven.

Each exact diagnostic passed once (3.582 and 2.246 s command). Remaining
integration targets passed 279 cases / six ignored, binaries/examples eleven,
and documentation tests zero cases. Workspace build passed in 222.786 s.
Every guarded file hash stayed equal. The full suite was not rerun to obtain
a green result; diagnostic passes do not erase its failure. Exact commands,
logs and hashes: `/tmp/fourth-keep-negation-native-*` and
`/tmp/fourth-keep-negation-remaining-*`.

The carry then rebased to 138624c7996ea986c3260b1f2cf0ebaee3f5382f on
train 767, b3e2d0903fb9a9a5fd34c41d84af761f9db3413c. Its one carried patch
is range-diff equal and every addition/deletion in all six repair files was
preserved. The first sandboxed rebase could not write the worktree index;
no rebase started or source changed. The permitted retry completed.
Current-base formatting passed in 19.371 s and strict workspace/all-target
Clippy in 182.382 s. Focused Engine/module tests passed 23 / zero failed or
ignored (2.00 s body; 213.898 s command). Integration passed 279 / six ignored
(101.735 s command), and workspace build passed in 106.539 s. All six
guarded file hashes remained identical throughout these gates. Commands,
logs and hashes are in `/tmp/fourth-keep-negation-train767-receipt.json`. Fresh full ARM CI is required,
under joining.md's unchanged timed-test rule; no older native or CI run is
relabeled as testing this runtime. This is not ready or activated.

### First-use timing of the added-negation path

A separate probe measures the new negation branch on its first call, rather than the A-to-B early-return path used by the mixed-case probe's first measurement. Both source epochs use “Remember that I like tea” / “Doesn't like tea.” The first result changes from true (incorrect acceptance) to false. First / median / p99 / maximum elapsed time across the first call and 1,000 repeated calls: before 305.315 / 147.211 / 271.467 / 419.461 us; after 306.088 / 189.378 / 289.616 / 414.235 us. Both meet the 1 ms coverage-call target in this sample. The native gate was also running on this host; no scheduler or device inference. Process startup is outside the measured boundary. The mixed 22-case sample is retained separately, not replaced. Source hashes/commands/logs are in /tmp/fourth-keep-letter-probe/negation-path-receipt.json and negation-path-{before,after}.{rs,log}. The after-source hash matches the current production prefix exactly.

## Follow-up: absence expressed with without

Reviewer-lead's second read of published `4dd0326ca` identified the remaining
case “I like tea with milk” kept as “Likes tea without milk.” On the unpublished
c290 carry `08e08517e9`, the exact production-prefix probe reproduced this:
one mismatch among 28 cases, exit 1. Adding `without` to the existing negation
vocabulary makes all 28 pass, exit 0. No new comparison pass, I/O, task or state
is added. `No longer` already contains the recognized `no`; added, dropped
and unchanged controls cover that phrase without another production rule.

The probe uses the actual helpers described above and real linked libraries.
The timed first call is this new with/without case, before other invocations:
290.912 us before, 184.112 us after. Across 1,000 calls over the same 28 cases,
the respective median/p99/maximum values are 112.133/230.565/639.910 us and
111.760/197.386/368.130 us. Both samples meet the under-1 ms host-comparison
target; no speedup, process-startup, Engine, model, storage or device latency
is established. Logs and exact inputs remain in
`/tmp/fourth-keep-letter-probe/without-{before,after}*`.

- Before production-prefix SHA-256: `6976d7515e7365f8face365572fcacebef4415953461ebe8d231b88868a47dbe`.
- After production-prefix SHA-256: `31926a6f8b23552ea3ab7d0c752321dece5b7723fa69221f24b1f7270f9d33b4`.
- Actual helpers remain `03712f0d6227fde1f38961324c8226cd87ec2d0600e2037b6b39c86c063f5718`.

One module regression and two Engine regressions cover changed forms taking
exactly the switch-off continuation, and unchanged forms retaining the fixed
one-round answer. On `08e08517e9` plus this six-file repair, Mac Rust 1.96.0,
short TMPDIR, two build/test threads and no incremental/debug information:

- `cargo +1.96.0 fmt --all --check`: passed, 23.076 s.
- `cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings`:
  passed, 223.544 s.
- `cargo +1.96.0 test --offline -q -p lamp-voice --lib keep_at_once:: -- --nocapture --test-threads=1`:
  26 passed / zero failed or ignored, 5.05 s body / 250.430 s command.
- `cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2`:
  **failed**, 5,506 passed / seven failed / 18 ignored, 1,538.82 s body /
  1,695.873 s command, exit 101. All guarded hashes stayed fixed.

The full-library failures were:

- `coordinator::tests::termination::sigterm_retains_failure_and_acknowledged_synthetic_diagnostics`:
  signal child did not finish.
- `ring_night::tests::the_worker_rests_on_the_asleep_face_only_while_expression_sleeps`:
  asleep-frame assertion.
- `ring_unplug::tests::actual_resting_and_speaking_frames_show_asleep_before_receipt`:
  acknowledgement assertion.
- `second_provider::net::chat::tests::a_kept_client_keeps_one_idle_connection_per_host`:
  four connections instead of three.
- `second_provider::net::chat::tests::a_kept_client_lets_an_idle_connection_go_and_the_default_reuses_it`:
  three connections instead of two.
- `second_provider::tests::chat_second_try::both_silent_the_slow_line_comes_at_its_own_time`:
  three requests instead of two.
- `second_provider::tests::chat_second_try::off_no_second_try_goes`:
  two requests instead of one.

Those five source files are unchanged from c290. One exact diagnostic per
identity passed on the same source, using `--exact --nocapture --test-threads=1`.
That does not establish the causes or turn the full run green. No full-library
rerun followed. The separately unrun `--test it` target failed 276 passed /
one failed: the light-call test's sorted middle was 53.318822 ms against 25 ms.
The other integration targets passed two / six ignored, binaries/examples
eleven, doc tests zero. `cargo +1.96.0 build --offline -q --workspace` passed
in 164.626 s. The remaining controller returned 101 for the retained integration
failure even though its final build passed. All six guarded source hashes
stayed fixed; only this evidence note was then extended.

Raw full/remaining logs and receipts are retained under
`/tmp/fourth-keep-without-c290-*`. Changed-path controls and static checks pass,
but this is not a green full-native gate, readiness or activation claim. Fresh
published-head CI and delta reads are separate. The branch keeps the switch
off; #5972's proposed activation has an independent HOLD because its train
lacked even the earlier A-value and added-negation repairs. Mixed-clause and
symbolic-value limitations remain and must be considered before activation.
