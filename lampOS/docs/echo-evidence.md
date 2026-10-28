# Echo evidence

Scope: `crates/audio/src/evidence.rs`, its use in `EchoProcessor`, and the
offline replay. Nothing here changes admission, the wire or the device.

**Revised after the first device recording.** As first published the
observer reported nothing on it. The search, the fit and one field changed;
[what the recording showed](#the-first-device-recording) is below.

## Why

Admission accepts a turn on speech probability alone, and the speech detector
listens to the microphone after echo cancellation. Whatever of Lamp's own
voice the canceller has not removed looks to that detector exactly like a
person. One recorded run admitted a second turn 846.5 ms after the first reply
began, with nobody speaking ([input admission](input-admission.md)).

The detector cannot tell the two apart. Something that also sees what the
speaker was given can. This module is that something. It produces
measurements for an admission decision made elsewhere. It decides nothing.

## What it reports

`EchoProcessor::evidence()` returns an `EchoEvidence` for the capture block
just processed:

| Field | Meaning |
|---|---|
| `blocks` | Capture blocks since the canceller was built or reset |
| `render_active_blocks` | Of the last 50 render blocks (500 ms), how many carried non-silent accepted speaker PCM |
| `capture_level` | Microphone level before cancellation |
| `residual_level` | Level after cancellation: what the speech detector and the provider receive |
| `echo` | Present only while non-silent render is matched to the capture |

and inside `echo`:

| Field | Meaning |
|---|---|
| `render_level` | Level of the accepted render over the matched window |
| `capture_explained` | Percent of the capture's energy, before cancellation, that the render explains |
| `residual_explained` | The same for the cancelled signal. A lower figure than the truth on a device, often much lower |
| `residual_playback_floor` | Percent of the cancelled signal's energy that has to be playback by accounting alone |
| `alignment_age_blocks` | Blocks since the alignment last explained at least half the capture |
| `render_block_level` | Level of the render at the alignment over the newest capture block alone (10 ms) |
| `render_drop` | Tenths of a decibel `render_block_level` is under its own peak over the last 150 ms. Logged for #4035's tail guard; no decision reads it yet |

Levels are tenths of a decibel relative to full scale, over the last 120 ms,
about the mean, in the analysis band below roughly 2 kHz. They are not
full-band loudness.

What the speech detector lacks is a statement about the cancelled signal,
and there are two.

`residual_explained` fits the render to the cancelled signal directly. High
means what remains is still mostly Lamp's voice. **Low means nothing.** What a
canceller leaves is what a linear fit could not remove, and a second linear
fit explains little of it. On the device recording, with nobody speaking, it
read a median of 26 while the cancelled signal was louder than -30 dB.

`residual_playback_floor` does not fit the cancelled signal at all. The
cancelled signal cannot hold more of anything else than the capture did, and
all but `capture_explained` of the capture is the most that can be anything
else. Whatever the cancelled signal holds beyond that much has to be
playback. On the same blocks it read a median of 78. It is a floor: it is
short of the truth by whatever playback the fit of the capture missed, it is
zero whenever the unexplained part of the capture could cover the whole
cancelled signal, and it holds only while nothing after the canceller
amplifies. `EchoProcessor` applies no gain.

Whether a person spoke, and whether Lamp was addressed, none of this can
know.

**Absent is not "no echo".** `echo` is absent while the render is silent and
while it is active but not yet matched. A consumer must treat the second case
as undetermined: `render_active_blocks > 0` with `echo` absent.

## How

The observer sees the three streams the canceller sees, in the same order:
PCM the speaker device accepted, the microphone block, and the cancelled
block. Provider audio that was never accepted by the speaker never reaches it.

1. Both streams are reduced to 4 kHz by plain averaging (six render samples or
   four capture samples to one, then each with its predecessor). The two get
   the same frequency response.
2. The alignment is the difference between the two streams' sample counts, so
   it does not move when render blocks arrive in bursts ahead of capture.
3. **One delayed copy says where to look.** The last 120 ms of capture is
   correlated with the render at 32 alignments per block, from no delay back
   to 400 ms, and the best of them is put forward. A search that starts when
   the speaker starts after a silence begins at the short delays.
4. **A fit decides.** Sixteen delayed copies of the render, 4 ms of it, five
   of them at a shorter delay than the alignment, are fitted to the capture
   together by least squares. The share of the capture's energy that fit
   accounts for is `capture_explained`, and an alignment is adopted when it
   reaches half. The match is recorded at the strongest single copy inside
   the fitted span.
5. Once held, the alignment's two neighbours are tried every block so slow
   clock drift is followed, and the search continues outward from it.
6. **Rivals.** A voice repeats every few milliseconds. An alignment one pitch
   period from the true one explains a steady vowel as well as the true one,
   and fails where the voice changes. Whenever the held alignment explains
   85 % or less, four alignments within 10 ms of it are fitted as well, all
   sixteen of them over four blocks, and one that explains 15 points more is
   taken instead.

One copy was the whole test as first published, with two neighbours fitted
afterwards. That is enough for a scripted path of one delay and one
reflection. It is not enough for a speaker beside a microphone.

A silent capture neither confirms nor unseats an alignment. A person speaking
over playback lowers the share and does not unseat it either; the age field
says how long ago it was last confirmed.

## Bounds

| Bound | Value |
|---|---|
| State | 4,096 render samples, two windows of 480 capture samples, a few counters. No allocation after construction |
| Work per capture block | 32 searched alignments and 3 tracked, one copy each over 480 samples; one sixteen-copy fit of the held alignment, shared by capture and cancelled signal; at most one more for a challenger and four for rivals |
| Measured cost, host, release build | 15.3 µs per block on playback alone, observer alone, one thread; 40.8 µs with the four rivals fitted every block |
| Measured cost, host, debug build | 0.71 ms per block |
| Measured cost, ARM64 binary under `qemu-aarch64`, release build | 88.6 µs per block on playback alone. Emulation, not the board |
| Delay searched | 0 to 400 ms from acceptance by the speaker device |
| Time to first match | 166 ms after the first echo in the scripted scene; 126 ms over the measured path; 120 ms after the echo became loud on the device recording |

No I/O, clock, lock or thread. Integer sums; floating point only for the
final ratios and levels.

## What was tested

Host, scripted scenes, `crates/audio/tests/it/evidence.rs`. Every signal is a
function of time sampled at 24 kHz for the speaker and 16 kHz for the
microphone. The echo path is a 113.5 ms delayed, inverted copy at half
amplitude with one reflection 4 ms later, plus a noise floor. The scenes run
through the real canceller unless marked. **This is a simplification of a
speaker beside a microphone, not a room, and none of it is acoustic
evidence.** The same tests pass with the same numbers as ARM64 binaries under
user-mode emulation.

| Scene | Result |
|---|---|
| Playback alone, 5.5 s | Matched 166 ms after the first echo; `capture_explained` median 95; no block below 50 in 5 s; nothing claimed before playback or after its last echo |
| A person alone | Never matched; `render_active_blocks` 0 throughout; levels -27.5 dB before and -29.5 dB after cancellation |
| A person over playback at -6, 0 and +6 dB relative to the echo | `capture_explained` 77, 49, 22 against 76, 48, 19 predicted from the mixture; `residual_explained` 4 in each: once the canceller has converged, what remains is the person |
| Scripted residual, observer alone: half the echo left in | `residual_explained` 95 with nobody speaking; 48 with a person as loud as the leak; 21 with a person 6 dB above it; 4 with no leak. `residual_playback_floor` 79, 40, 17 and 0, against a true 100, 50, 20 and 0 |
| **A voiced reply over the path the device recorded** | A pitched source, 195 Hz gliding 8 %, over the 21 coefficients measured below. Matched 126 ms after the first echo; `capture_explained` median 93, tenth percentile 91; no block below 50 in 5 s. The observer as first published: median 47, 431 of 500 blocks below 50 |
| Microphone clock 300 ppm fast for 20 s | Alignment followed; median 91 in the last 4.5 s; no block unmatched or stale |
| Render accepted in bursts 1 to 6 blocks ahead | Within 2 points of steady acceptance |
| Canceller reset | Evidence absent until the next capture, then counts from 1 with no match |

The canceller removed 10 dB from this synthetic echo before the first match,
so the recorded false interruption is not reproduced through it. The scripted
residual stands in for a canceller that has not converged.

Sixteen copies of an unrelated voice account for a few percent of any signal
by chance: 4 in the scripted scenes. Against the person's own speech in the
device recording, fitted offline with unrelated stretches of the reply, the
median was 2, the ninetieth percentile 10 and the ninety-ninth 29.

## The first device recording

First Rust pilot, `51bd815e9`, one question and one reply of 1.37 s, its
certified diagnostic bundle replayed on the host with
`lamp-audio replay-aec`. `recorded` is the device's own microphone and its
own cancelled signal. No device was touched and the recording stays outside
Git.

| | As first published | Now |
|---|---|---|
| Capture blocks, of which with the render active | 4,001, 157 | the same |
| Blocks with a match | **0** | 100 |
| First match | never | 120 ms after the echo became loud, 260 ms after the first non-silent render |
| `capture_explained` while the capture was above -40 dB, 85 blocks | | 91 / 94 / 99 at the tenth, fiftieth and ninetieth percentile; none below 50 |
| `residual_explained` while the cancelled signal was above -30 dB, 30 blocks | | median 26 |
| `residual_playback_floor` on those 30 blocks | | 27 / 78 / 93; 50 or more in 22 |
| Match claimed during the spoken question, or after the reply | | none, in 374 and in 3,470 blocks |

What the recording showed, measured outside the observer at its own rate:

- The room was at -77 dB before the reply and the microphone at -7.5 dB to
  -17 dB during it. Three samples of the capture were at full scale.
- One delayed copy of the render, at 104.25 ms, explains 30 % of the capture
  over the reply and half or more in 27 of 106 windows. Sixteen copies
  explain a median of 91 %. Ninety percent of the path's energy lies between
  103.75 ms and 108.25 ms, half of it by 104.5 ms.
- The voice repeated about every 5.25 ms. An alignment that much early
  explained 97 % to 99 % of the opening vowel, and the observer held it for
  220 ms before taking the true one. Before rivals were added, such an
  alignment was kept and fell from 94 % to 30 % in 200 ms.
- The device's canceller left the cancelled signal as loud as -11 dB. In the
  20 blocks where that signal was above -40 dB it was a median of 9.6 dB
  under the capture. Nobody was speaking.
- An envelope comparison of the cancelled signal with the render did not
  separate this leak from unrelated speech (median correlation 0.33 against
  0.03, with the ninetieth percentiles at 0.86 and 0.45). I did not pursue
  it.

One reply, one lamp, one room, one voice. The coefficients in the regression
are that lamp's.

## What is not established

- **No threshold.** Nothing here says at which share a turn should be
  refused. That needs recordings of real playback with and without a person
  speaking over it, at the desk, and none with its render reference is
  available off the device.
- **A real room, beyond one recording.** The fit spans 4 ms. A path spread
  further, another lamp, another voice or a higher pitch is untested. An
  earlier session measured a normalized correlation of -0.966 between
  microphone and accepted render at a single offset
  ([AEC alignment](aec-alignment.md)); the pilot's capture did not behave
  that way, and I have not reconciled the two.
- **Loudspeaker distortion.** Distortion products are not explained by the
  render and read as "something else". The pilot's capture touched full
  scale.
- **Low speech-to-echo ratios.** By the arithmetic of the mixture, a person
  10 dB under an echo that reads 94 moves `capture_explained` to about 86,
  and the floor then allows that all of a quiet cancelled signal is the
  person.
  Nothing here sees a quiet person under loud playback reliably.
- **A person over playback on a device.** Every double-talk figure above is
  scripted. The pilot has no moment with both.
- **Who is speaking and to whom.** Out of scope for any measurement of this
  kind.
- Device cost. The figures are from the host and from emulation. The native
  figure on the board is not measured.

## Scoring a recording

`lamp-audio replay-aec` now writes, on every `capture_pair` line of
`operations.jsonl`:

```json
"echo_evidence": {"recorded": {...}, "aec_only": {...}, "aec_ns": {...}}
```

`recorded` compares the microphone with the cancelled signal the device
itself produced in that run. The other two belong to the replayed cancellers.
Any existing diagnostic bundle with its accepted render can be scored this
way without new device software, including the recorded false interruption
once its raw files are at hand.

## Integration

Nothing on the wire carries this yet. The request is one optional field,
`MicrophoneFrame.echo: Option<lamp_audio::evidence::EchoEvidence>`, set by the
capture worker from `EchoProcessor::evidence()` after each `capture`. It
describes the frame it travels with, so that frame's freshness and lineage
rules cover it. See request `A1-003` in
[Agent 1's status](../coordination/agent-1.md).

`EchoMatch` gained `residual_playback_floor`. Nothing consumed the type yet.

How to read a frame, for whoever writes the admission rule:

- `echo` absent with `render_active_blocks > 0`: undetermined. The first
  reply after a canceller reset spends its first tenth of a second or so
  here.
- `capture_explained` high: the microphone is hearing Lamp. It says nothing
  about a quiet person underneath.
- `residual_playback_floor` high: the cancelled signal, the one the speech
  detector hears, is mostly Lamp. This is the field for a speech detector
  that has just fired during playback.
- `residual_playback_floor` low or zero: not determined. It is never
  evidence of a person.
- `residual_explained` high agrees with the floor. Low is not evidence.

A reset of the canceller resets the observer. The alignment therefore has to
be found again after every DSP epoch change, which takes one search pass once
playback resumes.

With `LAMP_QUIET_EVIDENCE=on` (off unless set; `skip_while_render_silent`)
the search is not run on a block while every render sample it would read, the
last 520 ms, is zero: it could match nothing then. The evidence is the same
block for block: no echo, the held alignment's age in `silent_at_echo`, and
the search's place moved on as the search would have moved it. A reset keeps
the switch, and the counts of what it skipped (`skipped()`). See
[the voice's runtime](serve-runtime.md#a-quieter-pace-while-nothing-happens-lamp_quiet_paceon)
for why (the capture worker's CPU in a loud room).
