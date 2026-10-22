# Search/push watchdog probe scope

The assignment is First's [Queue comment 6344036556](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6344036556).
This probe has no code change or PR. Its owned branch,
`tests/1569-search-head-probe-20261022`, points directly at fetched lamp-voice
`16b1a2a1582a0aeaaf8f3bbc5421800a1a51b3a6`.

All four #3046 issue comments and its empty inline-review comment list were
read; their external paths and hashes are in [external-records.json](external-records.json). In particular, Fourth's failure report is
[comment 6343967896](https://github.com/autonomous-ai/lampOS/pull/3046#issuecomment-6343967896);
Thirteen's later three passing seven-test Mac batches are
[comment 6344247207](https://github.com/autonomous-ai/lampOS/pull/3046#issuecomment-6344247207).
Those batches are prior reports, not executions performed by this probe.

## What failed previously

Fourth's preserved raw log reports the two assigned tests failing
`Run::assert_clean` at `second_route.rs:379`: `scenario host watchdog; verified
cleanup failed: session Fixture: fixture child ... exit observation timed out
(5 occurrences)`. Both contain `search_pushed`, `hand_control_cancel`, and
`turn_finished` events. The first also contains `look_dropped`; the second
contains the earlier `look_full_still` and its admitted turn. Those event
names alone do not establish every later assertion: `assert_clean` failed
before the test reached its scenario assertions.

The raw `(5 occurrences)` is an aggregated diagnostic count. It is not
evidence of five independent waits on the same child. The complete original
log is copied without modification in [fourth-original-output.txt.json](fourth-original-output.txt.json),
with its source path and SHA-256 in the same wrapper.
Fourth reported `108e360b1` plus an uncommitted speaker-proof correction;
that was not an unchanged-base comparison.

## Exact fixture boundaries at the pinned head

- `second_route.rs:2502` and `:2549` are the two tests. Each takes the existing
  `one_session` guard and runs both a pushed and an unpushed scenario.
- `pushed_search_answering` at `:2445` uses a private bus and scripts mute at
  capture frame 1,100. It leaves `Script::limit` at the 12-second default.
- `run` at `:1466` starts cooperative child processes first, then enters
  `scenario_clock::Scope::enter(limit * 2)` at `:1593`. Each scenario therefore
  has a 24-second real host watchdog after child startup. Its coordinator and
  fixture peers advance together on the existing scenario clock.
- `Peer::close_input` at `:452` drops the child's input and observes its actual
  exit, using the scope's host deadline. `scenario_clock.rs:261` calls
  `waitid(EXITED | NOWAIT | NOHANG)`; it probes before checking that deadline.
  It does not consume the exit, so the supervisor must still reap the child.
- `assert_clean` requires the scripted physical mute, successful reaping and
  Stop observations for all peers, and no forced shutdown.

The test file, scenario clock, and `pilot_faults.rs` containing `one_session`
are byte-identical between committed `108e360b1` and the pinned probe head;
see [source-identity.json](source-identity.json). Other committed Voice files
changed between those heads, and Fourth also had uncommitted changes. This
comparison does not prove whole-tree equivalence or the cause of the timeout.

## Declared executions and limits

Mac: one exact execution of each test, each in its own process, native
`Darwin x86_64`, debug profile with debug information and incremental build
disabled. The compile uses two jobs and the offline registry. The independent
SMALL target was reserved for this probe; MAIN Update filesystem work was
concurrent, recorded as context only in [mac-results.json](mac-results.json).

ARM64: one existing `full=true` dispatch, run #6055 / 39804008782. The workflow
has no targeted filter input. It runs release tests in four Linux ARM64
shards, one test process at a time per shard. The two exact primary nextest
rows were extracted once each from terminal shard logs. This is not a run containing
only those two tests. No workflow edit, SSH, local ARM build, rerun, or manual
issue/PR comment is part of this probe. The existing automatic full-run
failure reporter is part of the authorized dispatch.

Passing isolated runs cannot by themselves establish why Fourth's full-run
watchdog expired. No fixed deadline, cleanup assertion, or product behavior
has been changed to obtain a result.
