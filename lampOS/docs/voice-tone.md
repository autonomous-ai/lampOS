# How a voice sounds: the voice half of "reads your mood"

Status: built, off unless `LAMP_VOICE_TONE=on` (with `LAMP_BUS=on`), not run
on a lamp. The row is "Reads your mood from your face or voice" (Ten's, the
face half; this is the voice half, #1722 at 07:46–07:49 on 21 Oct).

For each line a person says to the lamp, the voice tells **how it sounded
against that person's own way of speaking**:

- **arousal**, calm (-1) to lively (+1), from the line's prosody;
- **valence**, down (-1) to glad (+1), from the line's own words, when a
  word says it;
- **how sure** the arousal is.

It goes on `voice/tone` ([bus.md](bus.md), [msgs.md](msgs.md)) and in the
line's `local_endpoint` and `voice_tone` session records. Numbers only: no
sound is kept past the line, and the words are read, never kept.

## Why prosody and words, not a model

What reads mood from a voice, and what fits a lamp (researched 21 Oct, #1722
at 07:48):

| Option | Size and speed | What it says | Licence | Verdict |
|---|---|---|---|---|
| audEERING's Wav2Small ([arXiv 2408.13920](https://arxiv.org/abs/2408.13920)) | tens of thousands of parameters; ~9 ms for 5 s on a Xeon; ~120 KB ONNX | arousal, dominance, valence | CC BY-NC-SA 4.0, research only; commercial from audEERING | The best fit, if bought. The first builder: not yet, first see whether prosody and words carry the row on the lamp |
| audEERING's w2v2-L-robust (MSP-Podcast, dimensional) | 165M parameters | the same | CC BY-NC-SA 4.0 | Too big, and the same licence |
| emotion2vec+ base or large (V1's, on the perception service) | 90M+ parameters | 9 categories | custom "model-license" | Too big for the lamp's CPU; categories, not a scale |
| Hume AI prosody and vocal bursts | cloud | 48 expressions | API sunset June 2026 | Gone |
| **The lamp's own prosody** (here) | under 1 ms a line | arousal | ours | Built: arousal reads well from level, pitch, its movement and pace; valence badly |
| **The line's own words** (here) | a word list | valence | ours | Built: English only |

V1 sent each session's audio to the perception service's emotion2vec. That
isn't taken here: it sends a person's voice to a server for every line.

## How it reads a line (`crate::voice_tone`)

- **The meter** (`ToneMeter`) takes each 10 ms block the detector calls
  speech, keeping only the last 30 ms of sound and a number or two a block:
  - **level:** the block's RMS, in dBFS; the line's is the median;
  - **pitch:** each 30 ms frame's normalized autocorrelation over 80–400 Hz;
    voiced at 0.6 or over; the top of the first peak within 90% of the best,
    so the octave below isn't taken. The line's is the median, and its
    **spread** the quartiles' distance, in semitones;
  - **pace:** peaks of the smoothed energy at least 80 ms apart and over 1.5
    times its median, a second of speech: a syllable rate.
  - A line under 0.3 s of speech or 10 voiced frames isn't read.
- **The baseline** (`Baseline`) is the person's own way of speaking: each
  measure's running middle over their lines, settling over 10 and sure after
  5. A line is read against it before it's taken in: steps of 6 dB of level,
  3 semitones of pitch, 2 of spread and 1.5 syllables a second, weighted
  0.35, 0.30, 0.20 and 0.15, through `tanh`. A loud speaker's own loud
  lines are their calm. **One baseline for the lamp:** at a desk that is
  mostly its one person. A baseline each, by the known face, comes with the
  face half.
- **Valence** (`valence_of`): glad and down words from a small list, a
  "not" or "never" in the two words before turning one by half; the mean,
  or nothing when no word says it.
- **When:** the meter is read at the line's End and the baseline takes it in
  then; the tone is told when the line's words come (the second route's
  final words, `Transcript` with no request), with their valence. A line
  whose words never come is told without them at the next End. An End whose
  request never went (a sound, a line given up) clears the meter.

## What reads it

`voice/tone` is the voice's. What the inner state ([mood.md](mood.md)) and
the skills make of it, beside the face's reading, is the face half's to
settle with this one (#1722).

## Tests

`voice_tone::tests`: a voice's pitch, its spread, its level and its pace read
near what was made; a line too short, or a hiss, says nothing; against a
person's own calm lines a livelier one reads lively and a calmer one calm,
the first line says nothing and the baseline is sure only after a few; a loud
speaker's own lines are their calm; the words' valence, turned by "not".
`lamp_msgs`: the message's round trip and its topic. The coordinator's
feeding of the meter, its End and its telling are not driven by a test.

## For a lamp

With `LAMP_VOICE_TONE=on` and the person at the desk:

1. ten ordinary lines first, for the baseline;
2. the same sentence read calm, then excited, three times each: the
   arousal's sign each time (`voice_tone` records);
3. a glad line and a down one: the valence's sign;
4. the voice part's CPU over the run, beside the same run off.
