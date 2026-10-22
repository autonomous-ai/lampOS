# Search/push watchdog probe, 22 October 2026

**The reported watchdog was not reproduced. Its cause remains unproven.**
This is an unchanged-source diagnostic, with no fix or PR, following
[First's assignment](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6344036556).
The owned branch `tests/1569-search-head-probe-20261022` points directly at
`16b1a2a1582a0aeaaf8f3bbc5421800a1a51b3a6`, fetched from lamp-voice at
15:20 UTC. No test, product code, deadline, assertion or workflow was changed.
Publishing these receipts on lamp-tests does not change the probe branch.

Each full name below starts with `coordinator::tests::second_route::`.

| Test | Native x86_64 Mac, one exact process | Native Linux ARM64, one nextest process |
|---|---|---|
| `a_push_mid_search_drops_its_looks_picture` | PASS; 4.14 s libtest, 6.120607 s process wall | PASS; 0.941 s; shard 3 |
| `a_push_once_the_looks_picture_went_cuts_its_answer` | PASS; 3.85 s libtest, 3.933973 s process wall | PASS; 2.240 s; shard 4 |

The Mac result is **two passed / zero failed**, one execution per test in
separate processes. Each exact process reported one passed and 3,390 filtered.
Both pushed and unpushed scenarios passed all original cleanup and search
assertions. No full Mac suite or retry was run by this probe. Compilation was
separate, passed in 208.402971 s, and used two jobs with the offline registry,
debug information disabled and incremental compilation disabled. The native
SMALL target was released after the two observations. Concurrent MAIN Update
filesystem work is recorded as context, not as a cause.

The existing workflow has no targeted-filter input. The single
[ARM64 full dispatch #6055](https://github.com/autonomous-ai/lampOS/actions/runs/39804008782)
therefore ran the workspace in four serial shards, one process per test per
shard, using the release profile. This was not a run containing only the two
assigned tests. Both exact rows occur once before their shard's summary.

| Shard | Actual job / direct check ID | Passed | Nextest elapsed | Terminal result |
|---|---:|---:|---:|---|
| 1 | 119912246928 | 2,527, including one passing leaky | 139.770 s | SUCCESS |
| 2 | 119912246681 | 2,618 | 193.045 s | SUCCESS |
| 3 | 119912246359 | 2,599 | 161.253 s | SUCCESS |
| 4 | 119912245857 | 2,751 | 188.772 s | SUCCESS |

The full nextest result is **10,495 passed / zero failed / 67 not run**.
Every shard reports the same 10,562-test inventory; 10,495 unique primary
PASS/LEAK rows are disjoint across shards, leaving 67 not run globally.
Per-shard skip counts include the other partitions and were not added.
The unrelated passing leaky row is
`lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
(0.217 s). It is retained, not called an ordinary pass or a target failure.
The doc/own-harness step and artifact steps also completed successfully.
Signing/publication and the automatic failure reporter were skipped.
All four actual jobs and their direct check-run records are terminal
`completed/success`; workflow aggregate success alone was not used.

[Fourth's original output](fourth-original-output.txt.json) is preserved
losslessly. That serial full run was at `108e360b1` plus an uncommitted
speaker-proof correction, with 3,383 passed / three failed / 14 ignored.
The two assigned cases failed `assert_clean` with the scenario host watchdog
and child-exit observation diagnostic. Their search/cancel events were present,
but the later scenario assertions had not run. `(5 occurrences)` is preserved
aggregated wording, not a count of independent waits on one child.

The [source audit](source-audit.md) identifies the 24-second host watchdog,
actual child-exit observation, and unchanged fixture boundaries. The fixture,
scenario-clock and one-session helper files match the earlier committed base;
other product files and Fourth's uncommitted context were not identical.
These isolated passes do not prove host-load causation, an inherited defect,
or a confirmed flake. They do not replace the earlier failed observations.
Issue disposition remains with the assigned reviewer/builder after this receipt.

[Source identity](source-identity.json) ties Mac and ARM to the same direct
commit and tree, not a synthetic PR merge. All 1,380 tracked inputs under
`lampOS/crates` plus the root Cargo manifest, lockfile and Rust toolchain file
matched committed blobs and remained byte-identical through the native
mtime refresh and executions. This is not a hash of the entire repository
or every external fixture. The probe worktree remained clean.

[Commands](commands.json), [Mac results](mac-results.json),
[Mac output](mac-output.txt.json), [ARM results](arm64-results.json), and
[selected original ARM lines](arm64-selected-output.txt.json) retain exact
commands, exits, times, source identities and terminal records. For every
raw-text wrapper, UTF-8 encoding `raw_text` recovers the stated original
bytes and SHA-256; the ARM wrapper explicitly contains selected lines only.
[External records](external-records.json) identify the full ARM logs, Cargo
machine messages and other omitted local files by path, bytes and SHA-256.
Those external files are not portable repository artifacts. No script,
binary, Cargo machine-message stream, device action, local ARM64 build,
manual Queue/issue/PR comment or new test execution is included in packaging.
