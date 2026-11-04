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

## Not verified

- **Needs-input line said once:** not shown (optional).
- On 3 November the planned cut-short retell and the hand-over right after
  a news were not shown because of the provider outage. Both were shown on
  4 November (below).

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

## 4 November: planned cut-short retell and hand-over after a news

Run on lamp-c0a4 from 09:20 to 09:43 (lamp time), with a synthetic voice
played from the Mac as the person. Cloud stayed on and no volume was changed
(the lamp Speaker stayed at 40%). The chat service answered throughout.

### Build

A local Docker ARM64 build (not CI) of `e9601f4111`, PR #5118's first commit.
The PR's head, `542cc3503b`, changes only `tasks_now` path stripping and
docs, so the retell code is the same. The build has the 45 s close read
(`CLOSE_READ_TIMEOUT`).

| Binary | SHA-256 |
|---|---|
| lamp-voice | `5ae45f4ef69b3993e2527bb960a8f33fae3cf37ff3409b3563d0275cfed87ede` |
| lamp-tasks | `2495c43661c06fb41fd4a4e516df649fad4f6ebaab7f5eb01d3f5ffef7fd06f2` |
| lamp-turn-end | `2fae7df06df3ab3fbf55510b866240073b6230ac22b51a64c49f68895b82f5b1` |

### Install

The install matched 3 November: the same `<test>` folders, the same model
and runtime copies, and one drop-in per unit. The voice drop-in sets
`LAMP_TASKS_NOW=on` and `LAMP_TASK_NEWS_RETELL=on`. The tasks drop-in keeps
today's product flags, which now include `--set-up-later
--targets-from-machines claude`.

- **Before:** lamp-tasks `4c7a3c6f…`, lamp-voice `7973f309…` (every worker
  and `lamp-turn-end`), both active, NRestarts 0. Drop-ins:
  `exit-note.conf` only. `targets.json` `43ddde40…` (one target, `office`).
- **Installed at 09:20:22:** both units active, NRestarts 0. Every voice
  process ran from `<test>`, worker `second` had `--tasks-now`, and
  `listening_ready` came at 09:20:23.74.

### Steps

Each ask was handed over through the sockets, not by voice: `prepare` on
`request.sock`, then `confirm` on `voice.sock`, as `lampos`. The ask was
"Reply with the single word done. Do not change any files.", sent to the
`office` target.

1. **News waits for someone there.** Task 33 was confirmed at 09:22:02 and
   dispatch was accepted at 09:22:12 (`exit 0, 9.7 s`). The task was idle at
   09:22:29, but its news was not told for 18 min. The lamp has no eyes that
   say someone is there (no `/run/lampos-eyes/present`), and a socket
   hand-over says nothing. So the news waited for "something said within
   120 s" (`HEARD_WITHIN_US`), as designed.
2. To give it that, a line not addressed to the lamp ("Okay, let me check
   something.") was played at 09:40:19. The news was told 5 s later.
3. **"Hey Lamp, stop."** was played about 0.7 s after the news's first
   answer audio.
4. Then the Mac stayed quiet.

### A. Planned cut-short retell: shown on the first try

| Event (task 33) | At |
|---|---|
| line heard (request 2, 5 words) | 09:40:23.13 |
| news told: "no skill asked … said by the voice", then "telling news" (request 3) | 09:40:24.13 |
| first answer audio of the news (1,369 ms) | 09:40:25.51 |
| "Hey Lamp, stop." played | 09:40:26–:29 |
| `was cut short (user_interrupted): its harness stays open`; 6 of 18 words played (3,340 ms) | 09:40:29.59 |
| `was not heard whole: told once more at the next quiet pause` | 09:40:29.59 |
| stop heard as a line (request 4, 2 words); its fixed line played (`LAMP_SPOKEN_STOP_ENDS`) | 09:40:30.25 |
| news told again (request 5) | 09:40:47.03 (17.4 s after the cut, about 15 s after the stop line ended) |
| first answer audio of the retell (1,618 ms) | 09:40:48.65 |
| retell heard whole; `closing task 33's harness: try 1: Closed` | 09:41:07.25 |

**Unplanned second case (task 34).** It was told 3 s after it went idle,
because the stop had been heard within 120 s. The next "Okay, let me check
something." landed on it and cut it short.

| Event (task 34) | At |
|---|---|
| news told (request 6) | 09:41:35.88 |
| cut short (`user_interrupted`) after 3 of 16 words (2,000 ms) | 09:41:40.13 |
| told once more at the next quiet pause | 09:41:40.13 |
| line heard (request 7) and answered as "the first thing said after the lamp's own news" | 09:41:40.93 |
| news told again (request 8) | 09:41:58.14 |
| `closing task 34's harness: try 1: Closed` | 09:42:17.79 |

Each news was retold once, heard whole, and its harness closed. No
`retold` line and no "not heard whole twice" line appeared (C).

### B. Hand-over right after a news (#5060 measure)

The hand-over for task 34 started as soon as the retold news of task 33 was
over (3 s with no lamp-voice line, 09:40:54). At that point the close of
task 33 had not yet logged.

| Event | At |
|---|---|
| retold news of task 33 told | 09:40:47.03 |
| hand-over started (ssh + `prepare`) | 09:40:54 |
| `confirm` answered `ok` (31 ms) | 09:41:07 |
| `closing task 33's harness: try 1: Closed` | 09:41:07.25 |
| `dispatch accepted (exit 0, 7.4 s)` for task 34 | 09:41:15.42 (8.4 s after the confirm) |
| task 34 idle | 09:41:32.24 |

The close that was in flight still logged `Closed` on its first try, and the
hand-over made during it was accepted.

The 13 s from the start of the hand-over to the confirm was not split
further. It includes the ssh from the Mac and the `prepare` step, and it
ends within the same second as the close.

### Restore

At 09:43:06 both drop-ins were removed, systemd was reloaded, and both units
were restarted. The `<test>` folders were removed.

- lamp-tasks: `4c7a3c6f…` (`/opt/lampOS/tasks/4c7a3c6f…/lamp-tasks`).
- lamp-voice: `7973f309…`, as were every worker and `lamp-turn-end`.
- Both active, NRestarts 0, `listening_ready` at 09:43:09.23.
- Drop-ins: `exit-note.conf` only. `targets.json` unchanged (`43ddde40…`).

Every harness opened in this run (tasks 33 and 34) was closed by lamp-tasks.
None is left open.

The 4 November lines are appended to [journal.txt](journal.txt). They are
filtered to dispatch, idle, news, cut-short, retell, close, words heard,
first audio and start lines, with the host name replaced by `<lamp>`.
