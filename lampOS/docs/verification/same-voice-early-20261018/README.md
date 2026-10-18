# A line's print from its first 1.8 s: #829, 2026-10-18

Fourth's evidence for asking a line's voice print once 1.8 s of it is
heard, not at 3 s or at its end ([docs/same-voice.md](../../same-voice.md)).
Run on Fourth's Intel Mac (i7-10700K). Nothing was run on a lamp.
Synthetic voices only.

## Why

On lamp-4ace on 18 October at `6ea956900`, with the check on, scene 3's
two short lines of B's ("And what about tomorrow?", about 2.1 s as sent,
and the umbrella line, about 2.5 s) were asked for only at their ends and
printed after their answers' first output (#829, the first builder at
06:54; Fourth at 07:13). Asked at 1.8 s, while they are still heard, their
prints get the rest of the line, the 0.6 s end silence and Gemini's own
latency to come back.

## The clips

The 19 lamp-quiet clips of [same-voice-20261015](../same-voice-20261015/README.md)
(`clips.tsv`, `sounding` under 0.1), cut again from the same runs'
`capture-audio/post_aec.pcm16le` by the capture records' read times, from
each line's `plays.tsv` start:

| Variant | From | To |
|---|---|---|
| `full` | the line's start | its end: the cut of 15 October |
| `most` | 0.3 s before its start | 2.7 s after it, or its end and 0.6 s: today's rule, the first 3 s of the request as the worker gets it |
| `early` | 0.3 s before its start | 1.5 s after it, or its end and 0.6 s: the first 1.8 s |

`full` reproduces 15 October's `pairs.tsv` to a mean cosine difference of
0.016 (at most 0.081), the blocks chosen at the edges differing.

## What was run

`lamp-same-voice pairs campplus_en_voxceleb.onnx` on each variant
(`pairs-full.tsv`, `pairs-most.tsv`, `pairs-early.tsv`), and the equal
error as on 15 October: the cosine threshold swept from -0.5 to 1.0 in
steps of 0.005, where the share of same-voice pairs below it and of
different-voice pairs at or above it are closest, their mean. The cutting
and the sweep are Python scripts in Fourth's scratch folder, outside the
repository.

## What it showed

171 pairs, 75 of the same voice and 96 of different ones.

| Pairs | `full` | `most` (today) | `early` |
|---|---|---|---|
| All | 5.9% (0.290) | 12.8% (0.270) | 10.5% (0.260) |
| The shorter line 1.5 to 3 s (16 same, 29 different) | 0 errors | 6.6% | 4.8% |
| The shorter line under 1.5 s (38 same, 67 different) | 7.7% | 18.2% | 21.7% |
| At the product's bar, 0.25: same voice taken for another | 2 | 7 | 8 |
| At 0.25: another voice taken for the same | 17 | 20 | 16 |

- **At the bar's own pairs, both lines 1.5 s or longer** (37 of the same
  voice, 29 of two): `full` gives 15 October's figures, none of the 37
  taken for two voices (the lowest 0.48) and 19 of the 29 caught. `most`
  takes none (the lowest 0.28) and catches 22; `early` takes none (the
  lowest 0.32) and catches 22.
- **The first 1.8 s tells voices apart no worse than the first 3 s.** Of
  the three groups it is better in two and worse in one, and at the
  product's bar it takes one more same-voice pair for another and four
  fewer other-voice pairs for the same.
- **The first 1.5 s is too little.** Asking at the bar itself, 1.5 s of
  the request (0.3 s before the start to 1.2 s after it), would have given
  every print more time. But it took 6 of the 37 same-voice pairs for two
  voices (the lowest 0.14), and all 171 pairs had a 17.5% equal error
  (`pairs-least.tsv`). So the print is asked at 1.8 s.
- **Both are worse than the clean cut,** most of all for short lines: the
  pre-roll and the end silence in the clip. Leaving the 0.3 s pre-roll out
  of either did not help (all pairs: 12.2% for the first 1.5 s from the
  start, 15.3% for the first 3 s).
- **A print of up to 1.8 s took 131 ms** (median of 19, at most 162) on one thread of
  this Mac, and one of up to 3 s 159 ms (at most 229).

## Not shown

Real people, the lamp's microphone with the lamp speaking, the child's time
on the A733 (the voice's journal now gives it on any run with the check
on), more than three voices. The groups are small: one pair moves a share by 1 to 6
points.
