# A kept name through the second worker's callback

Part of #2540 and the “Remembers you” row on #2602. The follow-up was
[claimed before code](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330259458).
The memory row remains 0.40; this is a host integration regression, not
spoken product proof.

## Why this check

The existing client test exercised the keep marker directly, while the
name-retention replay recreated the refresh decision. Neither connected
the process's real memory client and scoped tool runner to the second
worker's actual context callback. A defect in that connection could be
missed even while both tests passed.

The worker's existing memory-bearing callback is extracted as
`memory_context`. Production and the regression use the same marker
consumption, mode selection and note refresh. Production retains its
remaining prompt composition. No refresh policy, switch default or
runtime diagnostic is added.

The tests use fresh child processes so the real `remember::start` globals
select their mode once, as they do in a worker. A local socket fixture
keeps each Days session's snapshot frozen. A confirmed synthetic keep
then requires a new session to make the updated note visible. The tests
dispatch through `scoped_tool_runner` and inspect the provider-facing
messages produced by the callback. This is direct callback and tool
integration, not a full worker IPC or provider execution test; its prompt
composition uses `system_message` with a fixed character and no external
time or facts.

Days coverage includes a stable room scope, a personal scope, a refused
keep, and held/revoked contexts that must leave a confirmed keep for an
admitted context. The stable room case has no intervening person change
to conceal a broken refresh. The legacy controls leave Days unset or use
the unrecognized value `ON`; both use the queued `kept` refresh and must
emit no Days refresh marker. A socket by itself does not select Days.

Nine synthetic exchanges over 368 simulated seconds evict the kept name
from both the six-exchange/300 s history and the eight-exchange thread
recap. The final messages must obtain the name from the memory note. The
fixture returns only the new name after the keep; it does not test the
real store's ordering of conflicting names, replacement policy, or the
model choosing the right name when speaking.

## Latency and validation boundary

The path is tool dispatch → confirmed keep → next admitted context →
provider-facing messages. A Days refresh retains its existing total
300 ms read budget. Legacy refresh remains asynchronous. The host cache
target is under 1 ms for the measured callback plus provider-message
serialization. The complete production callback also performs its
existing time, facts and thread work.

A negative control disabled consumption of the production keep marker.
Both Days cases failed their updated-note assertion, while the legacy
control passed. The worker was restored byte-for-byte, then all three
focused tests passed in four fresh child processes. This catches a broken
connection to the marker even without a person change.

The restored run measured the following on the Intel Mac. These are
single observations, with assertions outside the measured boundary:

| Callback plus provider-message JSON | First | Refreshed | Cached |
|---|---:|---:|---:|
| Personal Days scope | 1.346 ms | 0.611 ms | 0.065 ms |
| Room Days scope | 1.349 ms | 0.638 ms | 0.074 ms |

Legacy keep dispatch to the updated provider payload took 1.239/1.482 ms
for the unset/unrecognized controls. The complete production prompt also
has time, facts and thread work; these figures are not its full latency.

On base `6bb802490`, formatting, strict workspace Clippy and the
workspace build passed. The Voice, Memory, Cloud, Skills, Update and
Voice-eval targets passed **5,194 tests, zero failures, 23 ignored**.
The three focused checks repeat within that total, and their four child
processes are not counted as extra tests. Integration targets were run
serially from the outset; both test-enabled Voice examples and all six
packages' doctest commands passed. Operator examples with `test=false`
were not run.

Exact command results, source hashes, mutation checks and measurements
are recorded in [`validation.json`](validation.json). Raw checks stay in
`/private/tmp/fourth-kept-name-callback-checks/`. The PR's exact-head native
ARM64 and Clippy runs remain the readiness gate. No microphone, speaker,
real provider, device deployment or acoustic latency is exercised here.

## Rebase after review

The source rebased cleanly onto `fab69b0c4`, preserving the later seems-note
integration and single-face correction. The memory-selection body remains
byte-identical to this base; prompt composition is identical after removing
indentation. Formatting and strict workspace Clippy passed, and all three
callback regressions passed again (zero failed or ignored). These repeats
are not added to the original 5,194-test total. Updated source hashes and
commands are under `rebase_integration` in `validation.json`; the original
full host gate and prior native result remain historical. The rebased head
requires its own native ARM64 and Clippy qualification before readiness.

## Integration with the updated tool dispatcher

The first rebased head `a8abee38d` passed native ARM64 run
[`39714810652`](https://github.com/autonomous-ai/lampOS/actions/runs/39714810652):
**10,025 passed, zero failed, 63 skipped**, plus four doctests, 12 process
tests and `relay_signals`. The existing same-voice journal test was a leaky
pass. Clippy run
[`39714810633`](https://github.com/autonomous-ai/lampOS/actions/runs/39714810633)
passed. These ran on CI merge `6fbb93c8a` with base `f71a62f55`.

Later `2d1d35396` changed the scoped tool dispatcher and `run_call`, both
exercised by this regression. The source rebased cleanly again onto
`5c8b03a2e`. Memory selection remains byte-identical to that base and
production composition matches ignoring indentation. Formatting, strict
workspace Clippy and all three callback checks passed again, with zero
failures or ignored tests. These repeats are not added to the original
5,194-test total. `dispatcher_rebase` records the commands and source
hashes; `prior_native_qualification_after_first_rebase` retains the prior
native receipt and artifact. The new head needs fresh native gates.

## Separate lamp receipt

Seventh's initial run on lamp-52e6 used build `403b444d4`, at 07:56–08:07
ICT on 22 October: one reported keep, 12 chat requests, zero Days refresh
markers and failed recall after 368 s. The [subsequent preflight audit](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330442424)
found Days **off**, one voice PID with 181 journal lines, one boot since
07:48:12 and a 28 ms memory answer. The journal has no confirmed-keep
result field. The missing Days marker is expected on that legacy path.

That run is void as #2540 Days proof. Its legacy recall result is not
explained by the configuration finding.

The [Days-on rerun](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330610361),
08:28–08:40 on the same build, passed the marker gate: one keep and one
next-turn refresh marker at 08:31:49. Six-minute recall was unproven in that attempt.
Request 16 called recall, then began a second round with 18 messages; no
answer was observed in the 14 s capture window. [Putback replaced the
worker](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6330704311),
so that attempt was incomplete.

The [complete repeat](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6331136109)
on the same lamp and build, 09:08–09:21 ICT, passed with Days and Rules
verified on, unchanged MainPID/InvocationID at both ends, one keep and
one next-turn refresh marker at 09:12:27. Request 49 recalled correctly
after 6 min 17 s of continued talk. Session events and the person at the
lamp supply the result. The Mac mic captured nothing in its 60 s window
and is not the evidence source. [First accepted this case at 09:45](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6331423960).
It proves this retention case, not outage recovery, conflicting-name
ordering, or the whole proposed 0.90 matrix.
Fourth has no lamp loan. The journal and spoken words remain on the lamp.

## Integration after memory retries merged

#2774 merged as `fdf8914f3`. This extraction now uses its typed note retry
policy and its post-lock/post-read authority guard in both memory modes.
The 40-line selection is byte-identical to that parent; the 43-line
composition has the same tokens and string literals. Guard and Sessions
bytes remain unchanged. Reversing the extraction restores the entire
parent worker byte for byte, including after formatting and the test-file
rename. The 508-line fixture itself is unchanged.

The worker switch census correctly caught its private test-child selector
because `kept_name_callback.rs` did not end in `tests.rs`. That initial
run had four passes and one failure. The file is now
`worker/kept_name_callback_tests.rs`, with the same module/test identities;
no production environment grant or census exception was added.

Fresh formatting, strict workspace Clippy, **24 focused cases / 0 failed /
0 ignored**, and workspace build pass after the rename. These are three
callback, nine retry/authority, six typed-client, one repaired follows-face
fixture and five switch-census cases. Repeats are not added to historical
full-suite counts. `rebase_after_memory_retry` in the JSON records the
extraction audit, commands, current source hashes and retained failure.
Raw checks are under
`/private/tmp/fourth-kept-name-callback-checks/rebase-after-retry-20261022/`.

This is a conflict-aware extraction, not an unchanged whole-commit replay.
New-head native gates and source integration review remain separate from
the earlier no-finding reads and qualified heads.

The verified integration patch then rebased unchanged onto `01887391d`:
`eced053fe = b34bd8490`. Worker, Remember and census sources have no
intervening drift. The newer base adds an early-end fixture clock feature
and other independently owned changes; a fresh local gate and current-head
native runs are recorded separately in the PR. The final amendment is
evidence-only. No earlier execution is relabelled as the new head.

## Final rebase after the integration read

Reviewer-two found no P1/P2 in the conflict-aware integration on
`4451c0334` ([read](https://github.com/autonomous-ai/lampOS/pull/2782#issuecomment-6334329645)).
That exact head passed native ARM64 run
[`39730903538`](https://github.com/autonomous-ai/lampOS/actions/runs/39730903538):
**10,247 passed, zero failed, 65 skipped**, two doctests, 12 separate process
checks and relay signals. The unchanged same-voice case remained a leaky
pass. Clippy run `39730903401` passed. The CI merge was `aa9547274` on
base `532c98545`; the receipt remains attributed to that earlier head.

The reviewed patch then rebased cleanly once onto
`07614f85214f2464746bba860ac545505063585f`:
`4451c0334 = 7acfe1499` before this evidence-only amendment.
The current parent worker is byte-identical to the retry integration's
parent. Recomputed reversal restores that entire parent worker exactly;
selection is byte-identical and composition only adds four spaces per
line. The 508-line fixture and its module identity remain unchanged.
The retry policy, guard, Sessions, scoped runner and census retain the
parent's implementation.

Fresh formatting, strict workspace Clippy, **24 focused cases / zero
failed / zero ignored**, and workspace build passed on this parent.
These are the same 3 callback, 9 retry/authority, 6 typed-client, 1 repaired
fixture and 5 census checks, counted once and never added to earlier full
suite totals. `final_reviewed_rebase` records commands, timings, hashes and
the audit. Raw checks remain in
`/private/tmp/fourth-kept-name-callback-checks/rebase-07614f852-20261022/`.
New-head native gates are required separately. This unconditional callback
extraction remains First's merge decision; no lamp action was performed.
