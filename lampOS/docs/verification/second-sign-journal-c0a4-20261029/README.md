# The second sign in the journal, on lamp-c0a4 (29 Oct 2026)

Part of #3945. The voice now journals which second sign let a hearing of the
lamp's name count (`Attention::second_signs`), and never the words.

**Setup** (03:24–03:27Z):
- lamp-c0a4 runs the installed product units. lamp-voice comes from a local
  ARM64 build of `thirteen/second-sign-journal-3945` (sha256 `b32cc1a2…`),
  installed as a /run drop-in with `LAMP_NAME_SAID_TO_A_PERSON=on`. The unit
  already sets `LAMP_NAME_SECOND_SIGN=on` and `LAMP_ATTENTION=busy-near`.
- Lamp at 40%, Mac at 60%, cloud on. No person was there; a synthetic voice
  was played from the Mac, about 28 s apart (`plays.txt`, Mac clock in UTC).
  The lamp clock is UTC+7.
- The lamp was restored after.

`journal.txt` holds the lamp-voice and lamp-name lines, which never carry
the words.

| Played (UTC) | Request | Journal |
|---|---|---|
| 03:24:14 "Lamp, what time is it in London?" | 3 | `counts by: its words calling it`, answered |
| 03:24:42 "Ma'am, did you see my keys anywhere?" | 5 | `counts by: a conversation going on`, answered |
| 03:25:10 "Ma'am, could you pass me the salt please?" | 7 | `counts by: its words calling it, its hearing at the line's start`, answered |
| 03:25:39 "Lamp, what's the weather like tomorrow?" | 9 | answered: "the text shows it was called" (no hearing weighed) |
| 03:26:08 "Ma'am, did you see my keys anywhere?" | 12 | `counts for nothing alone`, then answered as the conversation going on |
