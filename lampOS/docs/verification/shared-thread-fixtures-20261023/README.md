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

The Mac gate and official PR ARM64/Linux checks will be appended with their
exact source identity and result counts. No lamp operation, acoustic result
or signed release is established here.
