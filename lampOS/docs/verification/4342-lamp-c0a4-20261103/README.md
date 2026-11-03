# #4342 lamp check on lamp-c0a4, 3 November 2026

Pieces 2+3 (`tasks_now`, `LAMP_TASKS_NOW=on`), the close-told double-news
fix, and piece 4 (`LAMP_TASK_NEWS_RETELL=on`), run on lamp-c0a4 with a
synthetic voice played from the Mac as the person. Cloud stayed on; no
volume was changed.

The chat model service had an outage throughout the run (HTTP 503/500 and
"thoughts are slow"; DeepSeek's open incident, fix in #5083). Most missed
tries below are that outage, not this change.

## Build

Local Docker ARM64 build (not CI) of base `7d34ece661` plus the uncommitted
pieces 2–4 and the double-news fix.

| Binary | SHA-256 |
|---|---|
| lamp-tasks | `a18470c27848f5d6791181fc667c6e8104d4e59ad3112990799e146f78d47ed5` |
| lamp-voice | `9591063523cf8aa1aeac793b04b075961ea4486fd0e8f728ed8ec73cf1a9992c` |
| lamp-turn-end | `2fae7df06df3ab3fbf55510b866240073b6230ac22b51a64c49f68895b82f5b1` |

## Install

- `/opt/lampOS/tasks/<test>/lamp-tasks` and
  `/opt/lampOS/voice/<test>/{lamp-voice,lamp-turn-end}`, with
  `libonnxruntime.so.1.30.0` and `smart-turn-v3.2-gpu.onnx` copied from the
  product voice dir (`ab06ad40…`), since workers run from the main exe's dir.
- One drop-in per unit (`zz-…-test.conf`) changing only `ExecStart` to the
  `<test>` path; the voice one adds `Environment=LAMP_TASKS_NOW=on` and
  `Environment=LAMP_TASK_NEWS_RETELL=on`. The tasks unit keeps the product
  flags (`--enable --watch --progress --push-wait --close-told
  --harness-as-person …`).
- Before: lamp-tasks `4a780296…`, lamp-voice `ab06ad40…`, both active,
  NRestarts 0, drop-ins `exit-note.conf` only.
- Attempt 1: installed 16:41:58, restored 16:49:05 (outage). Attempt 2:
  installed 16:54:09, restored 17:59:58. Both times: both units active,
  NRestarts 0, every voice worker and `lamp-turn-end` from `<test>`, worker
  `second` started with `--tasks-now`, `listening_ready` within 1 s.

## Scenario 1: `tasks_now` (pass)

Task 15 ("ask Claude Code on my computer to write a two-line poem about the
moon"): `confirm_agent_task` 17:01:26.34, `dispatch accepted (exit 0, 12.2 s)`
17:01:38.65.

| Said | At | Words heard | Tool called | Tasks part | First answer audio |
|---|---|---:|---|---:|---:|
| "Hey Lamp, what is Claude Code doing right now?" (task 15 working) | 17:01:55 | 9 | `tasks_now` | 17 ms | 2,130 ms |
| "Hey Lamp, how are my tasks going?" (task 15 closed) | 17:08:00 | 7 | `tasks_now` | 13 ms | 4,846 ms |

## Scenario 2: no second news after the close (pass)

| Event | At |
|---|---|
| news told (`…:15:13`), request 20 | 17:02:37.08 |
| not heard whole (`no_audio_answer`: chat had not begun in 3,000 ms) | 17:02:40.09 |
| "told once more at the next quiet pause" | 17:02:40.09 |
| news told again (request 21, first audio 2,412 ms, 235 chars) | 17:02:49.08 |
| `closing task 15's harness: Ok(Closed)` | 17:03:18.07 |
| further news for task 15 | none to 17:59:58 (56 min) |

Before the fix (product build, earlier today) task 11 was closed at
15:28:17 and its news told a second time at 15:31.

## Scenario 3: retell (partly shown)

**Shown, unplanned (task 15, above):** a done news whose telling ended
unheard (`no_audio_answer`) was offered again at once ("told once more at
the next quiet pause"), told 12 s later with someone there (a face in view),
heard, and its harness closed 29 s after. Only one retell; not told again
after that.

**Planned cut-short, not shown.** Task 17 (rain poem): `confirm_agent_task`
17:52:23.40, `dispatch accepted (exit 0, 10.2 s)` 17:52:33.71, news told
17:53:00.72 (request 47, first audio 17:53:02.54). "Hey Lamp, wait a
second." was played from 17:53:02, over the start of the news. The lamp
made no request from it (no line, no `cut short (user_interrupted)`). The
news turn ended at 17:53:14.91 with "was not heard whole
(audio_written_with_playback_gaps_unscored): its harness stays open". No
"told once more" line followed, and there was no retell or close in the
6 min watched (to 17:59:25). So a turn whose audio was written but scored
neither heard nor cut does not count as not heard for the retell
(`unheard = heard.is_empty() && !played`). Task 17's harness was left open
when the product units came back.

**Fixed after this run (not yet on a lamp).** A news played through with
gaps is not heard (#4035: its harness stays open), so it was left in
between: neither told again nor closed. With `LAMP_TASK_NEWS_RETELL=on` it
now counts as not heard for the retell (`unheard = heard.is_empty()`), so
it is told once more at the next quiet pause, within the same two tries.
The tests pass. Showing it on a lamp needs a chat service without the
outage.

## Ask phrasings (P2 count)

| Phrasing | Tries | `start_agent_task` | Model missed | Lost to the outage or the name |
|---|---:|---:|---:|---|
| "have Claude Code write a two-line poem about the moon" | 6 | 0 | 1 (request 9: answered, no call) | 5: 503 ×1, chat not begun ×1, name not heard ×3 |
| "ask Claude Code on my computer to write a two-line poem about …" | 7 | 3 | 0 | 4: 503 (requests 24, 27, 30, 36); and 1 of the 3 starts lost its yes to "thoughts are slow" (request 41) |
| "send a task to Claude Code on my computer: …" | 0 | — | — | not needed |

The yes ("Hey Lamp, yes, please send that task to Claude Code.") got
`confirm_agent_task` on both tries where the start had been called and the
service answered.

## Not verified (provider outage)

- **Planned cut-short retell** (`cut short (user_interrupted)`, then a
  retell at the next quiet pause): the over-talk was not heard as a line,
  and the s3 retries were capped at three rounds, 10 min apart, because of
  the outage.
- **Hand-over right after a news: confirm → dispatch accepted** (#5060
  measure, with a close in flight): not run, since it depended on the retell
  above. This binary has the 10 s close read, not #5060's 45 s.
- **Needs-input line said once:** not shown (optional).

## Restore

17:59:58: both drop-ins removed, daemon-reload, both units restarted.
lamp-tasks `/opt/lampOS/tasks/4a780296…/lamp-tasks`, lamp-voice
`/opt/lampOS/voice/ab06ad40…/lamp-voice` (every worker and `lamp-turn-end`
too), both active, NRestarts 0, `listening_ready` 18:00:01.24; drop-ins
`exit-note.conf` only; `<test>` folders removed.

## Files

- [journal.txt](journal.txt): lamp-voice + lamp-tasks lines from both
  attempts, filtered to tool calls, words heard, dispatch, news, retell,
  close, service errors and start lines; the host name replaced by `<lamp>`.
