# Recovering an incomplete memory note

Part of #2540 and the “Remembers you” row on #2602. Claimed and planned at
https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6329283914.
This is a second-route Days-client correctness change, with no new switch.
It does not change Friday’s #2673 handoff or #2510 replacement policy.
The retry applies to Days notes; the authority guard applies to leased
contexts in both memory modes. Legacy note loading gains no retry, and
unleased contexts retain their existing behavior.

## Failure and changed behavior

A successful `remember` marks the cached note for refresh (#2627). Before
this change, any rendered result of that refresh was cached for the rest
of the run, including an unavailable result after a refusal or timeout.
A successful room read beside a failed personal read was also frozen.
Recovery of the memory service did not invalidate either cache entry.

The client now reports whether every requested snapshot was validated,
separately from its prompt text. Empty snapshots are valid. An incomplete
result replaces the old text and becomes eligible for retry after 30 s;
the next context built then opens a unique new Days session. Successful
notes remain cached. Keeps and person changes bypass this retry delay;
confirmed forgets retain the existing journal-resume switch and backoff.
Rules and follow-up reads keep their existing behavior and shared budget.

A held change of person invalidates the old cache even when that held
context skips the read. Request authority is checked after taking the
session lock and again after building the context. A request revoked or
narrowed during the read returns no context. A revoked request that waited
for an older context does no new memory I/O.

## Latency boundaries

The interaction path is context build → synchronous note read → chat.
The existing total read budget remains **300 ms**, shared by room and
personal snapshots, optional rules and follow-ups. Cached contexts make
no note RPC. The target for the cache-only host measurement is **under
1 ms**; the socket read target is **within the existing 300 ms budget**.
Neither is a microphone-to-sound or ARM lamp measurement.

Repeated incomplete results are rate limited to one attempt per 30 s
from the attempt start; no background thread or polling is added. An
explicit keep or person change retains its existing immediate refresh.
The old session closes on the existing writer queue. Every attempt gets
a new session number, so an old queued close cannot close the retry.

## Evidence

The new worker tests drive cache behavior, the real Voice socket client,
and scope revocation/narrowing. In the name-recovery case, the store
acknowledges one synthetic name, refuses the following snapshot, then
recovers without another keep, person change, or restart. Eight later
exchanges over six virtual minutes remove the name from both the
six-exchange/300 s history and the eight-exchange thread note. Only the
recovered snapshot supplies it. This fixture controls store replies; it
is not the real store or a spoken lamp result.

Client tests validate empty snapshots, refused and malformed room and
personal snapshots, a held room timeout, a room read consuming 200 ms
before a held personal reply, and a successful concurrent keep whose
marker must survive completion of an older snapshot. A failed initial
sandboxed run could not bind its Unix socket; socket tests run outside
that restriction. The normal socket run passed before the final queued
cancellation and thread-eviction coverage was added.

On the rebased source at `a503d70f0`, the 9 worker and 6 client checks pass
with zero failures or ignored checks. On macOS 26.6.2, x86_64, Rust 1.96.0:

| Boundary | Wall time |
|---|---:|
| First socket note read → rendered result | 0.560 ms |
| Refused refresh → incomplete result | 0.104 ms |
| Cached note lookup (`Sessions::frozen`), no RPC | 0.001 ms |
| Recovery read → rendered result | 0.270 ms |
| Held room reply → timeout result | 300.192 ms |
| Room reply held 200 ms, then personal reply held → timeout result | 300.434 ms |

The two slow cases share a configured 300 ms I/O budget. Their full call
returns exceeded 300 ms by 0.192/0.434 ms of host overhead; the regression
allows 100 ms of scheduling tolerance. These are single host observations,
not a distribution or an acoustic measurement. Recovery adds one bounded
read on an eligible context; healthy contexts keep the cached path.

A negative control on the pre-rebase `e2b2f596d` source disabled only the
incomplete-note retry. The same real-client recovery test failed because
the name remained absent after the six virtual minutes. The source was
restored byte-for-byte. Its retry logic and regression are unchanged by
the clean rebase. The queued-authority test covers revocation/narrowing
while a competing context holds the lock; its start signal does not prove
an exact scheduler interleaving inside the lock call.

Workspace formatting, Clippy with warnings denied, and the workspace build
pass. The Voice, Memory, Voice-eval, Cloud, Skills and Update target groups
finish with **5,144 passed, 0 failed, 23 ignored**, counting each passing
group once. This includes Voice's two test-enabled examples; all six
packages' doctest commands pass with no doctests. The 15 focused checks
are repetitions within that total, not extra passing tests.

The initial full run exposed a mock that labeled a room snapshot as a
personal one. Its scope was corrected; the full Voice library then passed
3,092 checks, with 14 ignored, and workspace Clippy passed again. The
workspace build preceded that test-only fixture correction; production
sources were unchanged afterward.

Two unchanged integration tests failed on the parallel Mac runs. Voice's
`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
measured a 56.693456 ms median against its 25 ms limit. Voice-eval's
`relay_hardening::a_closed_runner_transport_stops_the_runtime_and_its_workers`
parsed an empty PID file during teardown. Their complete integration
binaries passed serially: 264 and 234 checks respectively. Neither test
nor its limit was changed. These serial passes do not erase the earlier
failures or qualify the native gate.

Exact commands, failed attempts, final component counts, times and source
hashes are in [`validation.json`](validation.json). Raw logs stay in
`/private/tmp/fourth-memory-note-retry-checks/`. A fresh native ARM64 run
and Clippy on the PR head remain the readiness gate.

## Rebase after the deterministic fixture correction

The source rebased cleanly onto `fc082681a`, which merged #2780:
`b18ff0bc6 = 28b1c3eaa`. Formatting, strict workspace Clippy and
16 focused checks passed, with zero failures or ignored checks.
These cover the nine retry/authority checks, six typed-client checks,
and the repaired follows-face fixture. The repeats are not added to the
historical 5,144-test total. `rebase_after_fixture_fix` in the JSON records
the commands and current source hashes. Raw rebase logs stay under
`/private/tmp/fourth-memory-note-retry-checks/after-follows-face-fix/`.

Both previous native attempts on `b18ff0bc6` remain recorded: each failed
the unchanged follows-face fixture after 7,877 passes, with 63 skipped and
2,131 selected tests unrun. No artifact was produced. The new head needs
its own native ARM64 and Clippy qualification; no failed result is waived.

## Separate product proof

The row stays at **0.40** until First accepts product evidence. Seventh's
[initial #2540 run](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330190386)
on lamp-52e6, build `403b444d4`, at 07:56–08:07 ICT reported one keep,
12 chat requests, zero Days refresh-marker lines and failed recall after
368 s. The [mode audit](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330442424)
then established that Days was off. The journal records a memory answer,
not a confirmed-keep result. A missing Days marker is expected on the
legacy path, so this run is void as Days proof. Its legacy recall result
remains unexplained. The earlier 07:18 attempt was also void.

The [Days-on rerun](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330610361),
08:28–08:40 on the same build, passed the next-turn marker gate: one keep,
one refresh marker at 08:31:49. Six-minute spoken recall remains unproven:
request 16 called recall and a second round contained 18 messages, with
no answer observed in the 14 s capture window. [Putback replaced the
worker](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330704311),
so that attempt was incomplete.

The [complete repeat](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6331136109)
on the same lamp and build, 09:08–09:21 ICT, passed with Days and Rules
verified on and unchanged MainPID/InvocationID at both ends. One keep,
one next-turn marker at 09:12:27; request 49 recalled correctly after
6 min 17 s of continued talk. Session events and the person at the lamp
supply the result; the Mac mic captured nothing in its 60 s window and
is not the evidence source. [First accepted the case at 09:45](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6331423960).
This qualifies that retention case, not outage recovery, conflicting-name
ordering, or the entire proposed 0.90 matrix.

No transcript is copied here; the journal stays on the lamp. Fourth has
no lamp loan. The separate callback regression connects the actual
context callback with the scoped tool runner, but does not explain the
legacy recall result or replace the spoken product gate.

The host recovery test does not prove identity handoff, correction or
forgetting, multilingual spoken recall, acoustic latency, or 0.90.
