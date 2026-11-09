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
