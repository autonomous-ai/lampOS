# First word after a pause: boundary audit

Read-only at train 77 product base `b2c9eb3dd`, following First's 15:41
hearing freeze. No new voice behavior, switch, source edit or lamp action.
No confirmed capture/STT packet P1/P2; the actual played loss remains
unattributed. Capture/coordination and provider/STT were traced independently.

## Three boundaries to discriminate

1. **Accepted onset prefix.** `crates/voice/src/activity.rs:13-25,594-630,
   662-665`: ordinary admission returns the last 30 ten-millisecond blocks
   and needs six consecutive scores at least 0.80. The product sets
   `LAMP_FIRST_WORDS=kept`, retaining a longer buffer, but `begin()` still
   returns the ordinary 300 ms unless yield/reply-end reach-back applies.
   A soft first word older than trigger minus 290 ms can be outside the
   accepted prefix when a louder second word finally triggers onset.
   This is a bounded-policy hypothesis, not evidence that the played
   utterance crossed that interval. Existing soft-start coverage tests a
   200 ms quiet opening, not arbitrary model onset delay.
2. **Capture stall/age rejection.** `audio/src/capture.rs:430-445,499-507`
   rejects slow reads and stale incomplete blocks; `audio_workers.rs:223-237`
   reports a gap and clears held frames, and coordinator:7737-7748 rejects
   frames aged 100 ms to 1.5 s. Microphone1/capture-clock hold adds 40 ms
   (`audio/src/reference/hold.rs:32,77-85`), reducing slack before the age
   threshold. Trace capture gaps, sequence/epoch continuity and frame age.
   Normal conversation changes do not close capture, its first block is
   sequence 1 without warmup discard, and the initial/post-stall control-data
   race already has an explicit preservation guard/regression. Ears' opt-in
   single-slot PCM export is separate; it is not established as this path.
3. **An intentional pending-answer release.** Existing
   `coordinator/tests/turn_taking.rs:1034-1056` expects the first piece to be
   lost for 0.7 s speech + 0.4 s quiet + 0.6 s speech (also 0.6+0.5+0.5).
   This specifically uses `LAMP_WAIT_KEEPS_ANSWER=on`, which the product sets.
   `while_answer_waits` (coordinator:705 onward) requires an answer to prior
   input awaiting first sound, or an already waited candidate, and hearing
   evidence. It excludes own-started hello/reminder/news/"hm?" with no input;
   a recognized name and name continuation bypass it. Do not generalize it
   to every pause. Its `input_let_go_answer_waits` reason and prior reply
   state discriminate this intentional loss from a capture prefix loss.

## Provider/STT findings and controls

`second_provider.rs:3281-3328,4273-4294` queues whole sample blocks in order;
backlog fails the turn explicitly. Opening STT is asynchronous while its
bounded input queue retains audio. `net/stt.rs:450-489,773-804` preserves
held quiet and sends it before resumed sound; a soft first word followed by
louder speech is not removed merely because the first word was below its
hold floor. A below-floor tail can still be discarded at end, as documented.
The product does not set the optional quiet-held switch.

Two existing regressions executed individually from the already-built
current local Voice test binary: pending-answer first-piece loss **1/0**,
ordered STT held-quiet/resumed-speech flush **1/0**. Executable SHA256 and
unchanged before/after binding are in existing-controls.json. No shared Cargo
writer was started. The first proves that narrow policy scenario; neither
runs an acoustic model or demonstrates the owner's failure.

V1 baseline inspected: `hal/drivers/voice/stt/deepgram.py:182-210` waits for
listener readiness then sends complete supplied chunks; autonomous.py:202-205
also sends the complete supplied byte chunk. The Rust queue similarly passes
whole blocks. No VAD/model/provider parity is inferred from this boundary.

## Recommended probe before a product change

Use First's already planned calibrated synthetic lines and matched product
configuration, retaining the existing frame/decision trace: compare raw/clean
PCM and block scores around "What" with candidate trigger sequence,
`input_admitted.prefix_first_host_read_us`, `capture_gap` and
`input_let_go_answer_waits`. Then compare accepted prefix to STT-send audio
and interim/final results. This separates pre-onset clipping, explicit wait
release, capture discontinuity, transport loss and model omission. Third
has no lamp and ran none of this. No person's raw audio belongs in Git.

Do not widen pre-roll or lower onset/wait thresholds from these hypotheses
alone: that can import earlier room/colleague words into a new request or
make coughs/room talk replace an answer. Keep request/privacy/stop fences,
measure first use and repetition at 1–2 m, and judge on the calibrated lines
then a real person as the freeze requires. First/Seventh retain product
hearing decisions and Second/Nine retain bare-name work.
