# The same-voice bar on lamp-4ace's mic2: #1445, 2026-10-18

The source of the bar for [docs/same-voice.md](../../same-voice.md)'s check
(#829), from recordings on lamp-4ace, scored on lamp-4ace by the first
builder with the sixth's script. All voices are synthetic (`say`), played
from the office Mac's loudspeaker; no person's voice is in any recording.
The cut clips were removed after scoring, and only the numbers are kept
here ([separation.tsv](separation.tsv)).

## Why

On lamp-4ace on 18 October one played speaker's prints ran from 0.14 to
0.89 against the conversation's voice, around today's bar of 0.25
(`DIFFERENT_BELOW`), which was set on clean synthetic clips off the lamp
([same-voice-20261015](../same-voice-20261015/README.md),
[same-voice-early-20261018](../same-voice-early-20261018/README.md)). The
first builder asked for both distributions from the lamp's own microphone,
the bar that separates them or that none does, and whether the low ones
are the prints with the lamp's voice in them (#705, 09:5x).

## What was run

- **The model:** CAM++ as `docs/same-voice.md` names it (SHA-256
  `357a834f…`), at `/opt/lampOS/same-voice/campplus.onnx` on lamp-4ace,
  through #1445's `lamp-same-voice pairs` (release `4b5c9cec…`), at nice 19.
- **The script** (Python 3, standard library only; lampOS is Rust only, so
  it is not in the repository: [joining.md](../../joining.md), and its text
  is on #1445, https://github.com/autonomous-ai/lampOS/pull/1445#issuecomment-6245306946,
  with the two fixes of 11:2x there). For each played line of a
  `--diagnostics` run it:
  1. found the run's capture session (the `events.jsonl` with `capture`
     records and its `post_aec.pcm16le`) and its render session (the one
     with `render_accepted` records);
  2. cut the line's print as the voice's worker does: from 0.3 s before the
     line's `plays.tsv` start to 1.5 s after it, or to its end + 0.6 s if
     sooner, taking each 10 ms block whose capture `read_completed_at_us`
     falls in that window;
  3. measured the lamp's own sound in that window: the share of it covered
     by non-silent render (each `render_accepted` record not marked as
     silence and not all zeros, heard from `accepted_at_us` +
     `queued_frames` / 24 kHz for its frames), and the tail, the render
     running without a break of more than one block from the window's
     start. A print is `quiet` with no render in it, `tail` with render
     only within its first 0.5 s from the start, and `over` otherwise;
  4. scored every pair with `lamp-same-voice pairs`, and for each group
     swept the cosine bar from −0.5 to 1.0 in steps of 0.005, taking the
     equal error where the share of same-voice pairs under the bar and of
     different-voice pairs at or over it are closest (their mean), as
     15 October did; and counted both errors at 0.25.

`plays.tsv` from `record_run.py` has no header; the copies scored were
given `start_us, duration_us, voice, kind, text[, volume]`, with `start_us`
on the lamp's monotonic clock, the clock of the capture records.

## The recordings

- **Older sets** (13–15 October, lamp-4ace mic2, mostly with the lamp
  quiet): `both-a`, `both-noagc`, `cut-a` to `cut-d`, `cut-q`,
  `cutab-night0151-6db` and `-now`, `set1`, `set2aa` to `set2af`. Voices
  Daniel, Fred, Karen, Moira, Rishi, Samantha and Tessa. 137 clips: quiet
  90, tail 1, over 46. `cut-q` was given twice, so its pairs were counted
  twice (the script now counts a folder once); that does not change the
  groups' picture.
- **The labelled set** (the sixth's scene list on #1445, recorded by the
  first builder with `directed-fixture --diagnostics` on mic2, the voice
  stopped): A, the lamp quiet (12:18–12:24, with a faint 0.3 s tone as its
  fixture); B, lines after the lamp's replies to Fred's questions in a live
  session (10:48–10:55), at 3.0, 3.5 and 4.0 s; C, A's lines at +6 and
  −6 dB. Daniel, Samantha and Karen. 89 clips, all `quiet` by the overlap
  measure.

## What it showed ([separation.tsv](separation.tsv))

| pairs | same voice | different voices | equal error | at 0.25 |
|---|---|---|---|---|
| **labelled, all (89, the lamp quiet)** | 950, median 0.62, lowest 0.27 | 2,966, median 0.23, highest 0.69 | **7.5 % at 0.455** | 0 same read as another; 1,269 different (43 %) read as the same |
| labelled, A against A | 84, median 0.60, lowest 0.40 | 192, median 0.26, highest 0.64 | 10.3 % at 0.50 | 0; 103 |
| labelled, B3.0 / B3.5 / B4.0 against A | 72 / 72 / 40, medians 0.55–0.59 | 144 / 144 / 80 | 12.8 / 14.2 / 5.6 % | 0; 74 / 77 / 34 |
| labelled, C +6 / −6 dB against A | 72 / 72, medians 0.59 / 0.57 | 144 / 144 | 6.9 / 11.8 % | 0; 64 / 79 |
| older, both quiet | 793, median 0.47, lowest 0.10 | 3,212, median 0.20, highest 0.69 | 14.3 % at 0.295 | 85 (11 %); 896 (28 %) |
| older, the lamp over one or both | 1,370, median 0.14, lowest −0.16 | 3,851, median 0.12, highest 0.62 | **47.9 % (chance)** | 1,048; 664 |

- **No bar separates the voices** in any group.
- **With the lamp quiet**, the labelled set's equal error is 7.5 %, at a
  bar of about **0.45**, and no same-voice pair fell under 0.27. The older
  sets give 14.3 %.
- **Today's bar of 0.25 is far too low:** it almost never says "another
  voice", and lets 43 % of different-voice pairs pass as the same.
- **With the lamp's voice in a print**, the model is at chance (47.9 %), as
  15 October found (47.1 %).
- **Set B did not make the echo-tail case:** the lamp's replies had ended
  3–4 s before each B line, so every B print was quiet. A tail set needs
  lines started during a reply's last second.

## What was decided (the first builder, #1445, 12:2x)

The check stays off and never holds an answer alone. If it is used, it is
one weak sign at about 0.45, beside facing, lips (#720) and the name, and
only on prints with no render inside them. Fourth moves `DIFFERENT_BELOW`
to 0.45 in #1467, still off.

## Also found, in the code

The conversation's voice is the print of the last line whose answer was let
out (`same_voice.rs`, `decided()` and `poll()`), so one mixed print becomes
the reference for every line after it. The first builder's decision
(#1445, 10:3x): judge a line by its highest cosine against every answered
print of the conversation (Fourth).

## Not shown

Real people; a print with only the lamp's tail in it (set B missed it); the
lamp over a line on the labelled set; any lamp but lamp-4ace.
