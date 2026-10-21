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
| **The lamp's own prosody** (here) | about 3.5 ms of one core for a 2 s line on a Mac's i7-10700K, a pitch every 30 ms; on a lamp not yet measured | arousal | ours | Built: arousal reads well from level, pitch, its movement and pace; valence badly |
| **The line's own words** (here) | a word list | valence | ours | Built: English only |

V1 sent each session's audio to the perception service's emotion2vec. That
isn't taken here: it sends a person's voice to a server for every line.

## How it reads a line (`crate::voice_tone`)

- **The meter** (`ToneMeter`) takes the 10 ms blocks the detector calls
  speech **while the lamp is silent**: the line's own, and the 400 ms just
  before it began, since its first syllable often comes before the turn
  does. Speech heard while the lamp's answer plays, earlier talk, and
  anything outside the line once it has begun aren't the line's (the
  reviewer's P2 on #2427). A line spoken over the answer is read from what
  was heard once the lamp stopped. The meter keeps only the last 30 ms of
  sound and a few numbers a block:
  - **level:** the block's RMS, in dBFS; the line's is the median;
  - **pitch:** every third block, the last 30 ms frame's normalized
    autocorrelation over 80–400 Hz, so the frames don't overlap; voiced at
    0.6 or over; the top of the first peak within 90% of the best, so the
    octave below isn't taken. The line's is the median, and its **spread**
    the quartiles' distance, in semitones. A gap between blocks starts the
    frame again;
  - **pace:** peaks of the smoothed energy at least 80 ms apart and over 1.5
    times its median, a second of speech: a syllable rate.
  - A line under 0.3 s of speech, or with under 0.3 s of it voiced (ten
    frames), isn't read. How sure it is grows to full at 0.6 s voiced.
  - **Cost:** about 3.5 ms of one core for a 2 s line on a Mac's i7-10700K
    (a standalone build of the pitch, 21 Oct). The lamp's cores are slower,
    and the lamp run's CPU item measures it there.
- **The baseline** (`Baseline`) is the person's own way of speaking: each
  measure's running middle over their lines, settling over 10 and sure after
  5. A line is read against it before it's taken in: steps of 6 dB of level,
  3 semitones of pitch, 2 of spread and 1.5 syllables a second, weighted
  0.35, 0.30, 0.20 and 0.15, through `tanh`. A loud speaker's own loud
  lines are their calm.
- **A baseline for each known person** (`Baselines`):
  - **Whose:** at each line's End, the eyes' known person at the lamp, read
    from their small file as the hello reads it, keyed by the memory person
    id, never the name.
  - **No known face:** a line goes to the room's baseline.
  - **Kept:** in memory only, at most 8 people; the one heard longest ago
    goes first. Nothing is written, and it's all gone at a restart.
  - **The records** say whose (`whose`: `person` or `room`), never who.
  - **The known person is the face at the lamp, not always the one
    speaking:** someone unknown talking beside them is read against theirs.
- **Forgets:** a line whose words ask to forget their face lets that
  person's baseline go, or the room's if no face is known. One that asks to
  forget everything lets every baseline go, the room's too.
  - "Their face" is the known face at the lamp at the line's End, as for
    whose the line is, not always the speaker's: someone unseen beside a
    known person who says "forget my face" lets the known person's baseline
    go, not their own (the reviewer's P3 on #2457). It's the same scope the
    memory's own forget uses, and only a tone baseline, learned again in a
    few lines.
  - The heard words decide, before any function runs, so a forget the lamp
    then holds back still lets it go; it is learned again in a few lines.
  - A forget from elsewhere (the app, another lamp) doesn't reach it, but
    nothing of it outlives a restart.
  - The `voice_tone_forgot` record says which, never whose words.
- **Valence** (`valence_of`): glad and down words from a small list, a
  "not", "never" or an n't word in the two words before turning one by
  half; the mean, or nothing when no word says it. "No" turns nothing: it's
  mostly an answer to the lamp ("No, I'm good", "No thanks").
- **When:** the meter is read at the line's End and the baseline takes it in
  then; the tone is told when the line's words come (the second route's
  final words, `Transcript` with no request), with their valence.
  - Words heard while another line is the lamp's are taken for that line's,
    so a reading still waiting for its own is told without them.
  - A line whose words never come is told without them at the next End.
  - Told without words, it's still recorded (`voice_tone`, `words: false`),
    so the records and the bus agree.
  - The heard words carry no turn. So a line's words that come only after
    the next line has ended are taken for that next line's.
  - An End whose request never went (a sound, a line given up) clears the
    meter.

## What reads it

`voice/tone` is the voice's. **Mood reads it**, with `LAMP_MOOD_TONE=on`,
beside how the near face seems (`humans/seems`, #2353): two topics, one per
sense, and Mood reads both (Ten's 10:47 on #1722).
- **A glad line** is a smile: a little energy and warmth, sharing the smile's
  cooldown.
- **A down line** is someone low: two minutes' quiet while a face is near.
  So someone low who says so is kept company even when the face reads
  `unsure`, as FER+ did for a held long face on lamp-75c9 (Ten, 11:20).
- **A surely lively voice** with neither adds half a smile's energy.
- **Each sense makes its own:** a line's tone neither adds to the face's run
  of reads nor breaks it.

The bars and the rest are in [mood.md](mood.md). The skills don't read it
yet.

## Tests

- **`voice_tone::tests`:**
  - a voice's pitch, its spread, its level and its pace read near what was
    made; a line too short, or a hiss, says nothing;
  - only the line's own speech and its 400 ms before are read: talk a second
    before it, or outside it once begun, changes nothing; speech running on
    into it gives its last 400 ms; speech with no line is never read; the
    pitch frames don't overlap;
  - against a person's own calm lines, a livelier one reads lively and a
    calmer one calm; the first line says nothing, and the baseline is sure
    only after a few; a loud speaker's own lines are their calm;
  - the words' valence, turned by "not", never by "no";
  - each known person read against their own way, and a line with no known
    face against the room's; a new person's first line says nothing;
  - at most 8 people, the one heard longest ago going first;
  - a forget of their face lets theirs go (the room's with none known), and
    of everything every one; which words ask it.
- **`coordinator::tests::second_route`** (a voiced fixture, the switch on):
  - a line spoken over the lamp's answer is read only from what was heard
    once the lamp fell silent (53 of its 60 blocks; taken while the answer
    played, 59);
  - each line's tone is told with its own words, under its own turn, and the
    second is read against the first.
- **`lamp_msgs`:** the message's round trip and its topic.

## For a lamp

With `LAMP_VOICE_TONE=on` and the person at the desk:

1. ten ordinary lines first, for the baseline;
2. the same sentence read calm, then excited, three times each: the
   arousal's sign each time (`voice_tone` records);
3. a glad line and a down one: the valence's sign;
4. the voice part's CPU over the run, beside the same run off.
