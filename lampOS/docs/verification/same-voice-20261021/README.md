# Another voice as a sign of someone else, on lamp-4ace's mic2: 2026-10-21

The evidence for the bar the same-one rules hold on (`LAMP_GOING_ON_NOT_OWN`,
`LAMP_QUIET_GOING_ON_SAME_ONE`; #2339, #2341): a line is held as someone
else's only on a print **under 0.30** against the caller's, not under the
conversation check's 0.45 (`DIFFERENT_BELOW`, [same-voice.md](../../same-voice.md)).
All voices are synthetic (`say`), played from the office Mac's loudspeaker in
an empty room. No person's voice is in any recording. The cut clips were
removed after scoring, and only the numbers are kept here
([separation.tsv](separation.tsv)).

## Why

At 01:03 on 21 October both same-one rules held one person's own follow-ups
on lamp-4ace (6 of 12 day lines answered), because played voices have no
face and the rules read "nobody seen" as a sign. They are to hold only on a
sign that it's someone else, and another voice is the one sign the lamp has
without a face. The question was how often one person's own line reads as
another voice, which would make the lamp ignore them.

## What was run

- **The lines:** the played test's two-person scenes (#829), 15 lines,
  Daniel and Samantha, `say -r 185`.
- **Clean renders** (the sixth builder, 02:3x): each line cut as the
  voice's worker cuts a print, 0.3 s of silence then the line's first 1.5 s,
  scored with `lamp-same-voice pairs` and CAM++ (`357a834f…`).
- **On mic2** (the first builder, 02:29–02:32): the same lines played to
  lamp-4ace with its voice `7dd16ccca295` serving as usual. `mic2/audio` and
  `voice/state` were recorded off the bus beside it (`lamp-bus record`), so
  the voice wasn't stopped and the lamp answered the calls.
  - Each print was cut from 0.3 s before the line's start (`scenario.py`'s
    `#PLAY_US`, lined up through `at_unix_ms`) to 1.5 s after, or to its
    end + 0.6 s if sooner.
  - A clip is `over` when `voice/state` said the lamp was speaking in its
    window, otherwise `quiet`.
  - The scores came from the same tool and the lamp's own
    `/opt/lampOS/same-voice/campplus.onnx`, its hash checked.

## What it says

- **Another voice never read as the same one:** 56 of 56 pairs on mic2
  were at 0.29 or under, quiet or not.
- **One person's own voice falls low when the lamp is speaking over it:**
  20 of 49 same-voice pairs were under 0.45. The voice never judges a line
  begun over the lamp's sound, so that doesn't reach the rules.
- **With both clips quiet,** the same voice was 0.35 and up, other voices
  0.25 and down. At 0.45, 3 of 20 same-voice pairs would hold the person's
  own line. At 0.30, none would.
- **The older sets with pairs kept** (the sixth builder, 02:4x, on #1722):
  on prints of lines of 1.5 s or more, 0.30 held none of 37 same-voice pairs
  and still caught 27 of 29 other-voice pairs.
- **The known cost:** 18 October's labelled set
  ([same-voice-20261018](../same-voice-20261018/README.md)) had one
  same-voice pair at 0.27, so a rare own follow-up can still be held at 0.30.
  The other cost runs the safe way: a few other voices pass as "same" and
  are answered.

## Limits

- Two synthetic voices, one room, one night. Real people's voices and a
  noisy room may score differently, and they need their own run before a
  rule holds on this sign where people live.
- Lines under 1.5 s get no print (`LEAST_SAMPLES`), so they never give a
  sign either way.
