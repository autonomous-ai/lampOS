# #5060 on lamp-c0a4: closing a told harness after a slow read (3 Nov 2026)

## What was tested

With `--close-told` on (the product unit), lamp-tasks closes a task's harness once the
lamp has told its done news. The close first reads the harness with `show`. Before
#5060 that read ran out at 10 s while `show` from this lamp takes 7 to 39 s, so the
close never happened (12:29 to 12:32 the same day: `closing task 7's harness:
Err("timeout")` three times). With #5060 the read gets 45 s within a 60 s close
budget, with up to 3 tries and a journal line per try.

## How it was taken

- **Binary:** a local ARM64 build of ae3e65911e (Docker, Debian), with CI run
  41594897073 ("lampOS on ARM64") green on the same head. `lamp-tasks` SHA-256
  `107bbdcf0c6182e7c17dec22eb42349e46d4ac1bdccc2af2652acb24fe96b389`, the same on
  the lamp after install.
- **Install:** the binary was copied to a test folder, `/opt/lampOS/tasks/<test>/lamp-tasks`
  (root, 755). One drop-in, `lamp-tasks.service.d/zz-<test>.conf`, held an
  empty `ExecStart=` and then the product ExecStart line with only the binary path
  changed. All flags were unchanged (`--enable --watch --progress --push-wait
  --close-told --harness-as-person …`). The install was followed by `daemon-reload`
  and a restart at 15:07:06 +07; the unit was active with NRestarts=0 and the exe
  was the test path.
- **Before:** the product binary was `/opt/lampOS/tasks/current/lamp-tasks` →
  `4a780296…`, active since 13:29:32, NRestarts=0, with only `exit-note.conf` as a
  drop-in.
- **Driving:** a synthetic voice played from the person's computer said the ask and
  the yes. The Mac played nothing while the news was told. Volumes were not changed.
- **Read back:** the lamp's journal (lamp-tasks plus lamp-voice tool and news lines,
  in `journal.txt`), the lamp-tasks store (tell and close times, read only), and
  `harness ps --json` on the person's computer.
- **Restore:** at 15:45:21 the drop-in was removed, followed by `daemon-reload` and a
  restart. The exe is `/opt/lampOS/tasks/4a780296…/lamp-tasks` again (through
  `current`), the unit is active with NRestarts=0, `exit-note.conf` is the only
  drop-in, and the test folder was removed. lamp-voice was not
  touched.

Test sentences: "Hey Lamp, have Claude Code write a two-line poem about the sea /
the moon.", "Hey Lamp, please start a new task for Claude Code on my computer:
write a two-line poem about the moon / the stars.", "Hey Lamp, yes, please send that
task to Claude Code."

## Results

The close time is from the store: from the moment the news was marked told to the
moment the close finished. It covers the `show` read plus `close`. The journal does
not time the read on its own.

| Run | Time (+07) | Dispatch accepted | News told in full | Read + close | Close result | Trial? |
|-----|------------|-------------------|-------------------|--------------|--------------|--------|
| 1 | 15:07–15:13 | 14.5 s (task 9) | no: cut short after 3 of 26 words by a 0-word line (`user_interrupted`) | none | none, so the harness stays open (by design) | no |
| 2 | 15:14–15:17 | none | n/a | n/a | n/a | no: no `start_agent_task` in 3 asks |
| 3 | 15:19–15:23 | none | n/a | n/a | n/a | no: no `start_agent_task`; a `confirm_agent_task` with nothing pending |
| **4** | 15:26–15:28 | 18.8 s (task 11) | yes, 15:27:50 → told 15:28:02 | **15,362 ms** (attempt 27 ms after told) | **`try 1: Closed`** 15:28:17 | **pass** |
| 5 | 15:30–15:36 | none | n/a | n/a | n/a | no: the start (task 13) expired without a confirm |
| **6** | 15:39–15:44 | 20.7 s (task 14) | yes, 15:43:44 → told 15:43:59 | **17,986 ms** (attempt 33 ms after told) | **`try 1: Closed`** 15:44:17 | **pass** |

Key lines:

```
15:28:17.866454 lamp-tasks: tasks: closing task 11's harness: try 1: Closed
15:44:17.170086 lamp-tasks: tasks: closing task 14's harness: try 1: Closed
```

From the store: task 11 has close `state=closed, tries=1, code=null`, and so does
task 14. Neither needed a second try, so the retry and give-up lines were not
exercised on the lamp.

On the person's computer, `harness ps --json` (Harness 0.5.30) no longer lists
`lamp-983d8442…` (task 11, agent `737d10b1-…`) or `lamp-81bf7bef…` (task 14, agent
`64af9c1d-…`). The harness from run 1, `lamp-cc366a4c…`, is still listed as idle,
because its news was cut short.

## Other findings (not #5060's)

1. **A 0-word barge-in cut the news (run 1).** Request 112 heard 0 words, yet it cut
   the news after 3 of 26 words (`user_interrupted`). The task stayed `told=false`,
   so its harness was left open.
2. **Second news after the lamp's own close (run 4 → 15:31).** After the close, the
   watch read task 11 again at about 15:30:00 and saw `stopped` (observation 10).
   At 15:31:19 the lamp then told a second news for the same task
   (`…:11:10`), which was "not heard whole". The `stopped` state that the lamp
   caused itself is reported as news.
3. **The hand-over is often not started.** Runs 2, 3 and 5 got no
   `start_agent_task` or no matching confirm (the known flake).
4. During run 6 a progress read still ran out at its 10 s limit (15:42:59) and a
   `wait` failed (`wait_failed`). Progress reads keep their 10 s limit; only the
   close's read changed.

## What was not shown

- The retry path (try 2 or 3) and the give-up line: both closes succeeded on try 1.
- The duration of the `show` read on its own. Only read plus close (15.4 s and
  18.0 s) is known from the store.
- A read longer than the old 10 s limit, seen alone. Read plus close was 15 to
  18 s; that is consistent with a slow read now fitting, but the split was not
  journaled.
- A news played through that was then followed by the person typing into the
  harness (`Left`).
