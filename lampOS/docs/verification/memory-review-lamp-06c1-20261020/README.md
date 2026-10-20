# The memory review on lamp-06c1, 2026-10-20 (#2176)

**What ran.** The memory review's first cases on a lamp: remember, recall, a correction, recall in a new session, a preference used later, forget, and a time or arithmetic line after a forget. They ran twice on lamp-06c1: on the Live route (build 3318, 13:28–13:50 +07) and on the second route (build 4142, 14:20–14:31 +07).

**How.**
- **The lines:** spoken by the Mac beside the lamp with `say`, at output volume 50. Two Live retries were at 45 and 85 (`1g`, `1i`, `1j`). Each line starts "Hey Lamp,". There was no person at the lamp.
- **The test lines:** an umbrella's colour (Live), a bicycle's colour and coffee without sugar (second route), the time and three plus four. They are in the table below, as said by the Mac.
- **How they were read:** `journalctl -u lamp-voice` and `journalctl -u lamp-memory` on the lamp, with precise times.
- **Counts and cleanup:** asked of the memory socket, `/run/lampos-memory/memory.sock`, as the memory's own user (`kept`, `search`, `status`, `forget`). A small socket client sent the JSON requests listed in `lampOS/docs/memory.md` "Requests", one line per request, and read each answer line. It is not kept here.

**The builds.**

| | Live, build 3318 | Second route, build 4142 |
|---|---|---|
| `lamp-voice` folder | `d6089a6a38f9…` | `4549abfcb6ee…` |
| `lamp-memory` folder | `0ce3bbfa071d…` | `f6fad265f98f…` |
| Route | `LAMP_VOICE_ROUTE` unset: Gemini Live | `LAMP_VOICE_ROUTE=second`: speech to text, DeepSeek chat, ElevenLabs `eleven_v4_turbo` |
| Memory flags | `--keep-days 30 --zone system --memory-days --bus-root /run/lampos-bus` | the same |
| Voice's memory switches | `LAMP_MEMORY_SOCKET`, `LAMP_MEMORY_DAYS=on`, `LAMP_MEMORY_RULES=on` | the same |

Each folder name is the sha256 of its binary. Neither unit had a drop-in. The full folder names are in `live-3318/0-builds.txt` and `second-4142/00-baseline.txt`.

**Said by the Mac.** Each line began "Hey Lamp,". The second route's start times are from the Mac's own log; the Live lines' times are the lamp's requests.

| Route | When (+07) | Said by the Mac | Case |
|---|---|---|---|
| Live | 13:31–13:42 (requests 65–84) | remember that my umbrella is violet (13 tries, the last reworded) | Remember |
| Live | 13:43:30 (request 85) | what colour is my umbrella? | Recall |
| Live | 13:44:27 (request 86) | my umbrella is green now | Correction |
| Live | 13:45:02 (request 88), 13:46:35 (request 89) | what colour is my umbrella? | Recall, then new session |
| Live | 13:47:23 (request 90) | asked it to forget the correction | Forget |
| Live | 13:48:05 (request 91) | what time is it? | A line after the forget |
| Second | 14:20:46 | please remember that my bicycle is red. | Remember |
| Second | 14:21:20, 14:22:16 | what colour is my bicycle? | Recall |
| Second | 14:23:08, 14:23:57 | actually my bicycle is blue now, not red. | Correction |
| Second | 14:25:05 | what colour is my bicycle? | Correction's recall |
| Second | 14:25:46 | remember that I like my coffee without sugar. | Preference |
| Second | 14:27:32 | can you make me a coffee suggestion? | Preference used later |
| Second | 14:28:08 | never mind, forget what I said about my bicycle. | Forget |
| Second | 14:28:46, 14:29:04 | what time is it? / what is three plus four? | Lines after the forget |

The Live lines are given by their content; their exact wording was not kept.

**The cases.**

| Case | Live, build 3318 | Second route, build 4142 |
|---|---|---|
| Remember | Called on the 13th line only (13:42:55, 26 ms). The 12 before it were held back as a busy room, or taken as not for it. Kept id 9. | Called on the first line (14:20:53, 25 ms). Kept id 22. |
| Recall | `search` found 1 (4 ms); answered. | First ask: the chat had not begun in 4.5 s, and it said it can't think right now. Second ask: `recall` found 1 (3 ms); answered, first audio 3.7 s after the words. |
| Correction | Called `remember`: kept id 11. Id 9 stayed: added, not replaced. Recall found 3 (5 ms). | Said twice: both called `remember`, kept ids 24 and 25. Id 22 stayed: added, not replaced. Recall made no memory call; first audio 1.7 s after the words. |
| New session | The conversation ended at 13:45:52 after the room went quiet. Recall at 13:46:39 found 4 (6 ms). | Not run on its own. The conversation ended at 14:25:58, 1.3 s after an answer, not after a quiet. |
| Preference used later | Not run. | Kept id 27. A coffee suggestion 98 s later made no memory call. |
| Forget | `forget_last` removed id 11. Id 9 stayed. | `forget_last` removed id 27, not 22, 24 or 25. The conversation end after it was refused (`bad_request`). |
| A line after the forget | The time was asked and told (13:48:07); the next line was answered. | The time and three plus four were answered. No exchange was appended after the refused end. |

**Findings from the answers and kept text.** These are stated once here; the files hold only ids, ops, counts and times.
- Live: id 9 read "My umbrella is violet" and id 11 "My umbrella is green". The first recall answered violet; the recall after the correction and the one in the new session answered green. After the forget, id 9 (violet) was the one left.
- Second route: id 22 read "Their bicycle is red": the person became "their". The recall answered "Red". After the correction, the recall answered "Blue" without asking the memory. Id 27 read "They like their coffee without sugar", and the coffee suggestion gave a no-sugar answer. "Forget what I said about my bicycle" removed the coffee entry, not the bicycle's.
- The two entries from before the test, ids 17 and 18, both read "My favorite food is jasmine".
- Speech to text heard the wake phrase as other words in several lines (no room text is kept here).

The summaries are `live-3318/0-summary.txt` and `second-4142/STATE.txt`. The second route paused at 14:24: the lamp's speaker was off by accident, so the first correction may not have played (`02-correction-journal-paused.txt`).

**What was put back.**
- The test's own kept entries were removed by id. On Live: `forget` removed 2, and the store was back to its baseline (8 exchanges, 0 kept, 0 rules). On the second route: ids 22 (3 removed), 24 and 25, and the store was back to its baseline (14 exchanges, 2 kept).
- Ids 17 and 18 were kept for a recall the next day.
- `lamp-voice` was restarted at 13:50 and again after 14:30, to end the journal the review had stopped.
- After the Live route, the Mac's volume was put back to 60, as it was at the start.

**Kept here.** Ops, counts and the lamp's fixed log phrases only, with these changes from the readings as taken:
- every `heard`, `said` and `text` field is left out of the memory listings, which keep ids, kinds, times, the person class and source ids (`live-3318/*-store.txt`, `*-kept*.txt`, `second-4142/01-today.txt`, `06-cleanup.txt`, `02-correction-journal-paused.txt`);
- the time tool's answer is left out of `5b-after-forget.txt`; the relay's address and another lamp's name are `<redacted>`;
- the Mac's own line log is the table above, not a file;
- one Live retry's file is renamed `1f-remember-retry5.txt`;
- the shell helpers and the socket client are left out.

**Not shown.**
- `probe-tools` 10/10: it needs a direct DeepSeek key, and the lamp has only the relay's credential.
- Recall from an earlier day: ids 17 and 18 are kept for it.
- Per-person memory: blocked by finding 9 of #2176. Every entry here is `person=everyone`, and no face was known.
- Power loss.
