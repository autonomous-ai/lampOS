# Fixed Mac parallel Voice observations, 22 October 2026

Ready for lamp-voice: **no, #3023**. One unchanged-base library execution
passed; one candidate execution failed 18 tests. All seven assigned cases and
kept-start passed in both. The earlier full candidate Update consumer failure
also remains. No test was retried to obtain a passing result.

| Frozen arm | Head | Cargo exit | Passed / failed / ignored | Libtest time | Command wall time |
|---|---|---:|---:|---:|---:|
| Before | `ec5428a34c0f66688e54200473cf26e6c3dd2b88` | 0 | 3,384 / 0 / 14 | 276.28 s | 376.357434 s |
| After | `735b2cb5db21176a0ad80fcb42c48e739f8362fe` | 101 | 3,366 / 18 / 14 | 360.55 s | 511.770055 s |

Each arm ran `cargo test -p lamp-voice --lib --offline` once in a Mac execution
context reporting Darwin `x86_64` (`processor: i386`). Physical hardware ISA
and translation status were not established. This pair did not capture
`rustc -vV` or a Mach-O architecture inspection tied to its binary hashes; no
claim of physical Intel hardware or native ISA execution follows from uname.
`RUST_TEST_THREADS` was unset: this is default libtest CPU parallelism,
not serial execution or nextest process isolation. Cargo used four compiler
jobs, dev/test debug 0, incremental 0 and the same explicitly reserved target.
The process reports 16 logical CPUs. Wall times include compilation;
they are not comparable test-latency measurements. Light-call is integration
coverage and **was not executed by these library commands**. No Linux ARM64 qualification or device
execution is included in this receipt.

Both freshly compiled libraries list the same **3,398 unique test identities**.
Each raw run contains all 3,398 terminal rows, including the three passed
`- should panic` rows. The before binary SHA256 is
`f3972a8a1019627418229df914e583cfb833d8f243558ad0bbc26b49021305b2`;
the after binary is
`3b1a1e4233d456984a3011b02db9281ff87b12bece8ddc584856bc7f99803cf0`.
Fresh compile lines name the correct worktree; artifact timestamps follow
command start. Commands, exact assigned rows, inventories and bounded cleanup
are recorded in [results.json](results.json). Each command's owned process
session was empty at cleanup; no leftover child needed termination.

The [18 complete failure blocks](failure-diagnostics.json) comprise one
body-motion `Still`, nine motor preflight timeouts, three stepped-motor child
timeouts, one motor child/control-loss timeout, two ring restart ticks over the
original 50 ms bound, one duplex `StaleOwner`, and one failed SIGINT child.
The SIGINT child's original error is lost in an error-type downcast; its
`Option::unwrap` panic and subsequent connection-reset errors are preserved
in [complete stderr](raw/after-library-stderr.json). It cannot be labeled
`StaleOwner` from this output.

[Source reachability](source-audit.md) found no direct clock leakage or new
global-lock dependency into those failures. This is not proof of no coupling:
indirect scheduling, child startup and filesystem effects remain possible.
The EC pass versus candidate failures does not establish an inherited fault,
a host-load cause or patch causation. Shared-host load samples and reported
concurrent workload are retained as context, not controlled variables.

The external runner originally exited 1 **after the baseline Cargo command
had passed**: its parser omitted the `- should panic` annotation on three
passed tests. The corrected continuation validated all stored baseline rows
against the already captured inventory, then ran only the previously unrun
after arm. Its runner exit 0 means both observations were recorded, while the
after Cargo exit remains 101. [Parser recovery](parser-recovery.json) keeps the
original error, hashes and exact three rows separately from test failures.

A preceding, single exact EC Update observation selected the same five Cargo
packages as the failed full consumer gate, to preserve feature selection.
Exactly one Round test ran: **1 passed, 105 filtered**, 28.46 s test time;
other harnesses had zero matches. Both the exact command and full streams
are retained. Its Round binary hash differs from the failed candidate's;
[artifact comparison](update-artifact-comparison.json) does not establish
compiled identity or fault attribution. The earlier full gate's **1,916 / 1 /
11** and original two-second updater-exit failure remain in the
[prior receipt](../voice-latest-refresh-20261022/). This focused pass does not
clear that gate.

The before/after worktrees stayed clean with zero source mutations. All
Git-tracked lampOS inputs, including docs, Cargo and toolchain files, had their
mtimes refreshed when switching trees, with identical content hashes before
and after. [Restoration](source-restoration.json) preserves both finalizers.
The complete 6,422-file before map is [source-before.json](source-before.json).
Apply the explicit removals and changed/added entries in
[source-after-delta.json](source-after-delta.json) to reconstruct the complete
6,517-file after map. Aggregate hashes use Python's default JSON encoding of
the sorted path-to-SHA256 map, as recorded in the restoration metadata.

[Raw stream index](raw/index.json) points to lossless UTF-8 line-array wrappers.
Join `text_lines` without separators and encode UTF-8 to recover each original
stream, including blank lines and trailing whitespace; verify its byte count
and SHA256. Both inventory outputs are byte-identical and share one wrapper.
No runner code, executable or Cargo machine-JSON is published.
[Provenance](provenance.json) retains external paths, source/runner hashes,
environment and limitations; [manifest](manifest.json) hashes every receipt
file except itself. [Validation](validation.json) records package checks.
The separate current-upstream observation and current PR CI disposition are
not part of this fixed pair; no subsequent result is implied here.
