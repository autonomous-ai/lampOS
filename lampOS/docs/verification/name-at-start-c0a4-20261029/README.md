# #4226 on lamp-c0a4: 8327's and 8389's own lamp-voice, the opening "Lamp" after desk talk

28 Oct 2026 21:26 to 21:53 UTC (29 Oct 04:26 to 04:53 in the journals' +0700), Thirteen. No person was there: the voices are synthetic, played from the Mac.

- **Builds:** c0a4's installed lamp-voice was 8327's own (sha256 `a77df0b6…`). 8389's own (`597b92b3…`, checked against its release manifest) went in through a /run drop-in.
- **Fixed in both arms:** `lamp-name` and every other part. Product switches, including `LAMP_ATTENTION=busy-near`, `LAMP_NAME_SECOND_SIGN=on` and `LAMP_VOICE_ROUTE=second`.
- **Order:** A B A B, five trials each. lamp-voice was restarted at each switch.
- **Each trial** (`plays.txt`: when it started, and when the last line began):
  1. Two colleagues' lines: "Did you finish the report for Friday's meeting?", "Almost. I just need to add last month's numbers.", "Great, send it to me when it's done."
  2. 1.5 s later, a third voice: "Lamp, I want to cook something new this weekend."
  3. About 55 s of quiet.
- **Volume:** the lamp's Speaker at 40%; the Mac unchanged.
- **Cloud:** lamp-cloud on. The chat provider answered HTTP 402 to every round, so "played" below is the verdict, not words.
- **Files:** `round-*.journal.txt`, lamp-voice's and lamp-name's journals for each round.

| Round | Build | Last line let out | Why the others weren't |
|---|---|---|---|
| a | 8327 | 4 of 5 | `lamp-name` near 0.848, under its bar |
| b | 8389 | 1 of 5 | heard 0.94 to 0.99, dropped "not at the line's start": 1686, 1712, 1722, 5554 ms |
| c | 8327 | 3 of 5 | dropped at 1822 ms; one with no hearing |
| d | 8389 | 5 of 5 | none |

**Totals:** 8327 7 of 10, 8389 6 of 10. Not a regression between the builds. 5 of the 7 misses were the name heard and then dropped by `attention::NAME_AT_THE_START_US` (1.3 s).

To read a round: `grep -E "its answer is played|counts for nothing alone|lamp-name: heard its name" round-b8389.journal.txt`.
