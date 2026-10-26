# Skills c5 interrupted-run audit

**Partial runtime evidence; no full-suite PASS.** Read-only audit of source,
compiler metadata, preserved stdout/stderr and frozen executables. No Cargo,
binary, `--list`, test, retry or cleanup was executed. The active target was
not accessed. Raw logs remain local; this report contains no absolute host
paths or raw log dump.

Source: `0c6fc02c7067b35905ba371cee04b29febb8d9c3`, base
`c5c7603a743690e291f85a664a7ee18624232581`. The full compile exited0.
Runtime was terminated by SIGTERM at the controller's disk reserve after
1393.498 seconds, exit−15; the owned direct child was reaped. This was not a
test assertion failure, a completed suite or an elapsed-time timeout.

The 53 serial Cargo Running markers bind to 53 stdout running-test blocks
and exact compiled artifacts/metadata targets. Of these, **52 completed
harnesses report 2,423 passed, 0 failed, 27 ignored**. Each completed block's
individual terminal case lines match its summary. There are no explicit
FAILED case lines or panic headers in the inspected streams.

All five compiled Skills harnesses completed:

| Target | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| lib `lamp_skills` | 157 | 0 | 0 |
| bin `lamp-skills` | 20 | 0 | 0 |
| test `it` | 312 | 0 | 1 |
| test `release_pair` | 2 | 0 | 8 |
| test `requested_audio` | 31 | 0 | 0 |
| **Skills total** | **522** | **0** | **9** |

The changed case `addon::says_the_judge_dropped_do_not_spend_the_hour`
appears exactly once as `ok` in the completed Skills `it` block. Its source
calls `a_fifth_say(true)`, asserts the fifth proposal and five-minute drop
refusal, then calls `a_fifth_say(false)` and asserts the four-per-hour refusal.
The passing test therefore establishes that both sequential branches and
assertions completed. This is one test observation, not two branch runs or
instrumented branch tracing. The source file and integration module wiring
match the recorded source snapshot and exact commit.

The unfinished block is **lamp-update / test `install`**, not lamp-setup.
It declares92 tests and has64 explicit `ok` lines, then the unterminated
`run_twice_it_changes_nothing` case, with no terminal result or harness
summary. Those64 partial-block passes are excluded from2,423 above. The27
remaining declared cases have no start/result lines in this block; no case
names or outcomes are invented. The interrupted case is neither PASS nor
FAIL.

All99 recorded executable bindings are equal before/after runtime. This
audit independently rehashed all99 retained frozen files and matched the
recorded hashes and sizes without reading the live target. Prior clone
receipts record independent inodes; live-target inode independence was not
rechecked here. The compiled artifacts contain67 test-profile executables;
14 have no Running marker in the retained stream. No inventory counts for
those executables are inferred.

`audit.json` retains a minimal subset: source hashes, raw-evidence hashes,
completed counts, interrupted-target binding and limits. The full local audit
also retains per-target executable bindings. Pairing relies on serial harness order across separate streams,
not shared timestamps. No doc-test invocation/result or subsequent workspace
build is established. These partial host results do not qualify a full
package/workspace gate or device behavior.
