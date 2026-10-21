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
  - **A forget memory does** (`memory/forgot`, the first builder at 10:5x on
    #2467), asked by the app or by voice, lets that person's baseline go
    too, or every one for every person's, and memory's `everyone` the
    room's. Read on the coordinator's loop at most twice a second
    (`bus_out::ForgotIn`), with `LAMP_BUS=on`; a rules scope (`r-`) is no
    voice. So a name forgotten in the app and enrolled again starts a fresh
    baseline. Nothing of it outlives a restart either way.
  - **A forget memory couldn't tell** (its bus not writable just then) is
    said in memory's journal and not told again. That person's baseline
    stays until the voice restarts or 8 other people are heard (the
    reviewer's note on #2488).
  - The `voice_tone_forgot` record says which (`all`, `whose`) and by what
    (`by`: `words` or `memory`), never whose words or id.
- **Valence** (`valence_of`): glad and down words from a small list, a
  "not", "never" or an n't word in the two words before turning one by
  half; the mean, or nothing when no word says it. "No" turns nothing: it's
  mostly an answer to the lamp ("No, I'm good", "No thanks").
- **When:** the meter is read at the line's End and the baseline takes it in
  then; the tone is told when the line's words come (the second route's
  final words, `Transcript` with no request), with their valence.
  - **The duplex route** hears a line in pieces and marks none finished
    (Gemini's input transcript, as in [policy.md](policy.md)). Its pieces
    are put end to end, up to 4 KiB, each line with the turn of the lamp's
    line it was heard in (`ToneLine`). A line is over as soon as that turn
    is no longer the lamp's line (the next line began, or it was let go),
    or once no piece has come for a second after its End. So pieces heard
    before a line's End wait for it, and two lines never join (the
    reviewer-lead's P2 on #2600). Its words go only with the reading of
    the line they were heard in, and the forget reads the same whole line
    (Thirteen's c0a4 run on #1722, 21 Oct: before this, a forget there
    never let the baseline go, and no duplex line was told with its words).
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

`voice/tone` is the voice's. What the inner state ([mood.md](mood.md)) and
the skills make of it, beside the face's reading, is the face half's to
settle with this one (#1722).

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
    second is read against the first;
  - a known person's baseline goes when they ask to forget their face;
  - the same, asked in pieces with none marked finished, as the duplex route
    hears it (`a_forget_heard_in_pieces_lets_the_baseline_go`);
  - pieces heard while a line is still being said are its words: kept until
    its End, and told with its reading, never the line before's
    (`pieces_heard_before_a_lines_end_are_its_words`);
  - a line begun while the one before's pieces still come never joins it:
    each is told with its own (`two_lines_in_a_row_each_keep_their_own_pieces`);
  - `ToneLine` alone: at most 4 KiB kept, a piece of another turn's line
    giving the line before back whole, and a line over at another turn or a
    second's quiet after its End, never while still being said;
  - a name memory forgot (`memory/forgot`) starts a fresh baseline when
    enrolled again, and a rules scope memory forgot changes nothing;
  - the loop reads memory's forget from a real bus (`ForgotIn` on a test
    bus, published after the first End), and the next line starts fresh.
- **`bus_out::tests::memorys_forgets_are_read_from_memory_only_at_most_twice_a_second`:**
  a person's forget and every person's, from memory only, well formed only.
- **`lamp_msgs`:** the messages' round trips and their topics.

## For a lamp

With `LAMP_VOICE_TONE=on` and the person at the desk:

1. ten ordinary lines first, for the baseline;
2. the same sentence read calm, then excited, three times each: the
   arousal's sign each time (`voice_tone` records);
3. a glad line and a down one: the valence's sign;
4. the voice part's CPU over the run, beside the same run off;
5. on the duplex route too, with a known face: "forget my face" is a
   `voice_tone_forgot` with `by: words`, the next line's `baseline_lines`
   is 1, and a glad line's `voice_tone` has `words: true`.
