# Memory snapshot retry boundaries

Part of the “Remembers you” row on #2602; the three regression follow-ups
from [First's #2774 P3 read](https://github.com/autonomous-ai/lampOS/pull/2774#issuecomment-6332940086).
Tests and verification only, on parent
`ae80e1c5d2b7bfdec22da7eaa326c68007934954`.
There is no production, API, configuration, switch, census or nextest change.
The existing #2774 retry policy and #2782 callback extraction stay unchanged.

## What runs

The three new tests use the production `Sessions` cache and the actual
`DaysClient` Unix-socket protocol. The socket controller runs on the test
thread; it has no background mock worker to stop or join. Scoped client
threads report through bounded channels. Accept readiness, request reads,
writes and result waits are bounded; held replies remain open until the
client returns, so closing the fixture cannot fake a timeout. Each test has
its own socket, removed on drop. Asynchronous `session_close` requests are
acknowledged separately and do not count as snapshot opens.

- **Timeout, then recovery without a keep.** Hold the first room snapshot
  reply through its configured 300 ms client deadline. At virtual 29 s the
  incomplete note stays cached without another read; at 30 s a new session
  returns a valid fact. The healthy result stays cached at 60 s. There are
  exactly two snapshot opens with distinct session identities and no keep.
- **A failed person, then another person before 30 s.** A's room succeeds
  but their personal snapshot is refused. At virtual 1 s, a normal non-held
  B lease goes through `told_while_scope_holds`; the captured snapshot's
  person feeds `Sessions::told_for`, and that actual change result feeds
  `Sessions::frozen`. No literal forced refresh is used. The room and B's
  personal note are read immediately. The old partial room note is absent,
  B's fact is present, and a repeat for B remains cached. Four opens, in two
  room/personal pairs with distinct session identities, are checked.
- **An eligible retry shares one budget.** Following a refused personal
  note, a retry at virtual 30 s spends 200 ms awaiting the room reply, then
  leaves the person's reply unanswered. The whole eligible ask is asserted
  to take at least 280 ms and less than 400 ms, for its explicitly configured
  300 ms total budget. Its next retry is scheduled from the attempt's start,
  at virtual 60 s; a call at 59 s must not read. Four snapshot opens are
  checked, with the same session identity within each room/personal pair.

Virtual `Instant`s move retry eligibility without a 30-second sleeper. The
300 ms I/O and the controller's 200 ms hold use the real host clock. The
100 ms upper tolerance is the existing client deadline-test tolerance; it
is not a production latency guarantee. Tests stay in the default nextest
selection, like the existing short client deadline controls. No test is
skipped or quarantined.

## Measurement boundary and limits

The elapsed retry observation starts immediately before the eligible
`Sessions::frozen` call and ends when it returns after
`DaysClient::session_note_within(..., Duration::from_millis(300), ...)`.
This verifies the explicit client API budget on the Sessions retry path.
The production `fresh_session_note` call's `FIRST_READ` wiring is checked
from source, not executed through global client initialization by these
new tests. The six existing client controls run beside them and include an
assertion that `FIRST_READ` is 300 ms and both scopes share its deadline.

The non-held test exercises the production lease guard and scope/cache
methods with local client state. It does not initialize the global
`memory_context` callback, drive a provider worker or test a real store.
The replies and facts are synthetic. No acoustic latency, lamp outage
recovery, spoken recall, face recognition or completed product-row score
is established. No lamp or person data was read.

Friday's #2510 corrections and #2673 follows-face work, and Twelve's #2894
follow-up acknowledgment work, are outside this change. First's separate
narrowing-mid-read invariant and disclosed rules/follow-up completeness
behavior remain unchanged.

## Gates and raw evidence

`validation.json` records the exact source hash, parent, commands, exit
codes, focused counts and elapsed I/O observation after the local gate.
Raw logs and the external gate runner remain under
`/private/tmp/fourth-memory-retry-boundaries-checks-20261022/`; the runner is
not committed. The focused groups count each test once: 12 retry worker,
6 client, 3 callback, 1 follows-face fixture and 5 census cases, 27 total.
Native ARM64 and x86 Clippy on the published head are separate PR gates.

The local gate passed **27 tests, zero failed, zero ignored**, formatting,
strict workspace Clippy and workspace build. The eligible two-scope retry
returned in **301,590 microseconds** against its explicitly configured
300,000 microseconds and asserted 280,000–400,000 microsecond host interval.
No local gate failed. New-head native checks remain required.

The final root read tightened the non-held case with a local loader counter:
1 after A, 2 after B, and still 2 after cached B. This closes the possibility
that an unnecessary loader call returned cached client data without another
accepted socket request. Formatting, strict workspace Clippy and that one
changed case passed again. The unchanged other 26 cases and workspace build
retain the initial source hash and execution attribution; the repeat is not
a 28th test. `loader_count_proof` records this final source hash and gate.
All 411 original test lines and all production/nextest hashes remain exact.

## Latest-base replay for review

The originally published head `3bbbb6cbc` passed native ARM64
[`39744314100`](https://github.com/autonomous-ai/lampOS/actions/runs/39744314100):
**10,334 passed, zero failed, 67 skipped**, four doctests, 12 separate process
checks and relay signals. Clippy run `39744313022` passed. One unchanged
same-voice test was a leaky pass. Its CI merge was `616ece571` on
`a16545d92`; artifact `12410858272` has archive SHA-256
`83c907ec12b04f7965e739cd49b097bc5505ed3f7d81cb38480f92640b6b7fbf`.
`qualified_head_before_rebase` preserves that old-head receipt.

Reviewer-two requested the latest authored ancestry while reading. The
one commit rebased cleanly onto `8dd69028a2799e1f534835911a404a5bdaab3e54`:
`3bbbb6cbc = 7e31615fd` before this evidence-only amendment. The entire
retry test source remains byte-identical, SHA-256
`c062891e2b4d977b6a01c00a43c02736e2453b9af3b0395e54084a4e466655ac`.
The current parent's 411 test lines remain the exact prefix. Worker,
Remember and nextest sources have no upstream drift since the old parent,
and this PR changes none of them.

Fresh formatting, **12 retry cases / zero failed / zero ignored**, strict
workspace Clippy and workspace build passed. The fresh eligible retry
observation was **300,779 microseconds**, with the same explicit
300,000 microsecond API budget and asserted 280,000–400,000 microsecond
host interval. Its boundary and production-wiring limitations above remain.
These repeats are not added to the original 27 unique focused cases.
`latest_base_rebase` records commands, hashes, timing and source audit.
Raw checks are in `/private/tmp/fourth-memory-retry-boundaries-checks-20261022/rebase-latest`. New-head native gates remain separate and are
pending publication; no earlier count qualifies an unpublished new head.

## Final replay after the Tasks fixture correction

Head `3c2d54877` retained a real failed native attempt:
[`39747538858`, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/39747538858/attempts/1)
ran **6,025 passes, one failure, 67 skipped**, with 4,337 selected tests
unrun and no artifact. The failed Tasks test expected a signed-out idle
diagnostic within five seconds; its last report was signed out but CLI busy.
No retry regression failed. [First explicitly authorized attempt 2](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6336615705).

[Attempt 2](https://github.com/autonomous-ai/lampOS/actions/runs/39747538858/attempts/2)
passed **10,363 tests, zero failed, 67 skipped**, with four doctests,
12 separate process checks and relay signals. One unchanged same-voice
case was a leaky pass. Exact-head Clippy `39747538905` passed. Both attempts
used CI merge `3891e6925` on `555117e75`; the rerun did not contain the later
Tasks correction. Artifact `12412589912` has archive SHA-256
`58be2a64faf1c01d2b590d83d1825162ed5462c862346d083ab1d6453bde5e7e`.
[Reviewer-two separately ran the full Voice gate](https://github.com/autonomous-ai/lampOS/pull/2936#issuecomment-6336623688)
on exact `3c2d54877`: **3,562 passed, zero failed, 14 ignored**. That is the
reviewer's execution, not another Fourth local total.

After #2954 merged, the one commit replayed cleanly onto
`2202bf2f56237f5547b37bdd777f3a3d66eed93f`:
`3c2d54877 = 570d89afc` before this evidence-only amendment. That parent
contains the Tasks fixture correction: the signed-out observation now
waits for two further sign-in reads instead of an idle report. The entire
retry test file is still byte-identical (`c062891e2b4d977b6a01c00a43c02736e2453b9af3b0395e54084a4e466655ac`),
its original 411-line prefix is exact, and relevant worker/Remember/nextest
sources have no drift. A file-only `rustfmt --edition 2024 --check` passed.
No Cargo run was repeated for this unchanged source; historical executions
keep their original provenance. Root publishes the final head and obtains
its fresh native checks separately. Earlier green heads do not qualify
that final unpublished head.

`qualified_3c_head_before_final_rebase` retains both native outcomes and the
reviewer's attribution. `final_rebase_after_tasks_fixture_fix` holds the
source audit. Raw audit and range-diffs are in
`/private/tmp/fourth-memory-retry-boundaries-checks-20261022/final-rebase/`.
