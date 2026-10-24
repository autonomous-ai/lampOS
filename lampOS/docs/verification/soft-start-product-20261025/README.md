# LAMP_SOFT_START in the product unit: lamp-4ace records, 25 October 2026

The first builder's records for turning `LAMP_SOFT_START=on` in the product and lamp-4ace voice units. All lines were
ElevenLabs voices played from the iMac beside lamp-4ace, with no person in the room (`night.py`, the owner's set: the
ten pause lines and the owner's two scenes).

## The third A/B (#3411's exact build)

The build was run 40130640911's `lampos-arm64-0b41ef8250711cec5c27e7b260fb5be28281821d` artifact, its `SHA256SUMS`
checked. It was installed on lamp-4ace's voice with 4ace's own drop-ins.
- **Arm 0:** both switches off.
- **Arm 1:** `LAMP_SOFT_START=on` and `LAMP_CALL_TAKES_NEXT_LINE=on`, from a `/run` drop-in.
- **Both arms:** `LAMP_FIRST_WORDS=kept` and `LAMP_SECOND_NAME_CARRIES=on`, which the call switch needs.
- **The switch read-back:** each arm's `LAMP_*` lines, read from the running voice's `/proc/<pid>/environ`, are in
  `ab3-flags-readback.txt`. So are the install, the restores and the light put back.

Strict means answered with WER at most 0.3.

| Round | Arm | Lines | Answered | Strict | Misses (id, WER) |
|---|---|---|---|---|---|
| `rP-0355` pause lines | 0 | 10 | 10 | 9 | q09 0.80 (answered as the bare call) |
| `rP-0412` pause lines | 1 | 10 | 10 | 10 | none |
| `rH0-0402` owner's set | 0 | 22 | 21 | 19 | q01 0.80, q10 0.80 (bare call); u10 0.14 held ("Louvre, …") |
| `rH1-0419` owner's set | 1 | 22 | 20 | 20 | u08 0.25 held ("Nang, …"); u10 0.14 held ("Louvre, …") |

Each line's row is in `ab3-arm{0,1}-{pause,owner}-*.txt`: what was said and heard, the lamp's reply and why it was played or held.

**Where each line's kept sound began.** `onset2.py` (in this folder, sha256 `31e3f06ed814b187…`) reads the pause
rounds' `mic2/audio` recordings (`ab3-arm{0,1}-onset.txt`). These figures are for the pause rounds' starts only. They
don't cover the owner's lines or latency.

| Arm | Starts | Lead-in min | Lead-in median (upper middle) | Under 100 ms | Trigger after onset, max | First word lost |
|---|---|---|---|---|---|---|
| 0 | 18 | −68 ms | 196 ms | 3 | 359 ms | 3 |
| 1 | 17 | +131 ms | 491 ms | 0 | 348 ms | 0 |

- **Arm 0 lost** "What did I just ask you?" (heard as "Did I…"), "What time is it in London?" ("Time is it…") and "Do I
  need an umbrella today?" ("I need…").
- **The arms changed both switches together**, so these figures don't isolate either one. The first-word figures go
  with `LAMP_SOFT_START`. The bare-call answers go with `LAMP_CALL_TAKES_NEXT_LINE`.

## The round on the exact product unit

- **Build:** lamp-voice `fc31e012334cda59eb3c489560454c515fb86386`, run 40134290386's artifact, sums checked. It
  carries #3411 and #3413.
- **Setup:** 4ace's voice was set as this branch's `deploy/product/lamp-voice.service` sets it, by a drop-in that
  `unit-dropin.py` made from that unit (`unit-round-dropin.conf`), with 4ace's own `zz-owner-hearing.conf` set aside.
- **What it leaves out** (the same six exceptions as #3371's F round):
  - kept as 4ace's own: `LAMP_SAY_GROUP` (a group), `LAMP_BUS_ROOT` and `LAMP_ONBOARDING_FILE` (paths);
  - not set, as 4ace's install lacks them: `LAMP_PERSON_NAME` and `LAMP_TASKS_NEEDS` (paths) and
    `LAMP_PRIVACY_CONTROLS` (a boolean).
- **The check:** before the round, the running voice's `LAMP_*` environment was compared with the unit switch by
  switch: "matches the unit (83 switches)" (`unit-round-envcheck.txt`).
  - A first try at 04:53 stopped on that check. Three switches that 4ace's own drop-in unsets came back when it was set
    aside, so the generator now takes every `LAMP_` name in 4ace's unit and drop-ins too.
- **The command line** (`unit-round-cmdline.txt`) differs from the unit's `ExecStart` only in the mute record path
  (4ace's own) and the unit's `--failure-notice-dir`/`--failure-notice-language`, which 4ace's install lacks.

| Round | Lines | Answered | Strict | WER median | First answer audio, median / max |
|---|---|---|---|---|---|
| `rUG-0454` | 22 | 22 | 22 | 0.00 | 1864 / 4733 ms |

- **Every question kept its first word.** Three rows' heard text omits the name: q03, q11 and q12, where the name was its
  own piece. u10, "Lamp, what did I just ask you?", was heard as said this time.
- **WER** throughout is the repository's played-test score (`tools/played/night.py`). It matches against the reference
  tokens and adds no penalty for inserted words.
- **What the time measures:** `night.py` reads the voice journal's `first answer audio N ms after the words were heard`.
  - In `second_provider::answer` that timer starts once the answer is begun with the words in hand. It stops when the
    first answer audio is handed to the output channel.
  - It doesn't measure the input's finalising (the endpoint and speech to text), or the sound reaching the room.
  - A soft start changes where a line's sound begins, which is before this timer starts.
- **The medians** are `night.py`'s, the upper middle of an even count. For `rUG-0454` that's 1864 ms; the middle two
  are 1832 and 1864, a midpoint of 1848.
- **Against the rounds on 4ace's own hearing setup**, on the same boundary and convention:
  - `rH1-0419`: 1486 ms (midpoint 1412);
  - `rH0-0402`: 1507 ms (midpoint 1507);
  - #3371's `rF-2312`: 1523 ms (midpoint 1468.5).
- **That comparison isolates nothing:**
  - this unit is lamp-voice's product unit as it stands, without #3371's relay routes and endpoint settings, and the
    F round had them;
  - one round of 22 lines can't separate a switch's effect from the route's.
