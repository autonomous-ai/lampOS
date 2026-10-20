# Issue 711 strip evidence and authored reconstruction

Source: [First's changed-mask comment](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6166294409),
lamp-4ace, 14 October 2026. First described MPR121 bus 0/address 0x5A,
50 Hz reads and the owner's hand. The comment contains **57 changed base
rows**, plus a separate earlier head GPIO trace. The base files here do not
combine the two time windows or claim simultaneous acquisition.

`strip-711-changes.jsonl` preserves all 57 base rows, their original wall
timestamp, pad list and literal source line. `mask` is a mechanical conversion
of that pad list. `at_us` is an **authored replay coordinate**: the wall-time
difference from the first row, multiplied by 1,000. It is not a recorded
monotonic clock, capture sequence or receipt timestamp. The source wall clock
has millisecond precision. No transport latency can be derived from it.

The first row at 17:27:09.236 is released. The first nonzero row is
17:27:22.158; there is no subsequent full release until 17:27:29.144. Thus the
changed rows describe one 6.986-second contact containing a hold, travel,
pauses and reversals. They do not establish acquisition continuity between
those changes. Replaying these 57 rows alone through the recognizer produces
eight explicit `SampleGap` errors and **no actions**, as required by its
100 ms continuity boundary. The complete coordinator/replay additionally fences
every acquisition interval and reports **nine Stale rejections**, with no
events. The pure recognizer's different count comes from reseeding after a
fault; neither path establishes a continuous contact. A fault is never a
synthetic release.

`strip-711-reconstruction.jsonl` contains **1,052 rows**:

- All **57 original rows**, unchanged and marked `recorded_change`.
- **995 authored constant-mask rows**, marked `authored_constant`, with null
  source timestamps, pad lists and source lines. These hold the last changed
  mask on a 20 ms grid, plus a final full-release sample 40 ms after the last
  original row. Original off-grid changes retain their exact replay times.

Its authored interval is 0 through **19,948,000 µs**. This reconstruction
assumes the unrecorded acquisitions stayed unchanged and valid; that is a test
assumption, not recovered hardware evidence. Tests use acquisition and receipt
times equal to these authored coordinates. A future full recorder trace must
retain actual acquisition timestamps, session/sequence and receipt times.

`strip-711-replay.jsonl` is the CLI-ready typed adaptation of those same 1,052
rows, in exactly the same order. It uses the explicitly authored clock and
reader session `strip-711-authored`, consecutive sequences 1 through 1,052,
and receipt equal to acquisition time. Match each row to
`strip-711-reconstruction.jsonl` for its original-versus-authored provenance;
the strict runtime replay schema intentionally does not accept extra metadata.
This adaptation creates no new hardware evidence. Use `replay --touch` with
the accompanying per-unit-independent configuration/pins examples and
`--clock-id strip-711-authored`; the final authored sample is 19,948,000 µs.

The original example retains the full E0-through-E11 order. First's later
[20:05 hardware answer](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6169047710)
confirms that E4/E8 are faulty on lamp-4ace. The new
`strip-711-working-controls.json` / `strip-711-working-pins-example.json` pair
selects its ten working pads, mask 3823 (0x0EEF). Sparse orders must be
ascending or descending. Positions retain all twelve physical slots: E5 is
5/11 in the ascending direction, not 4/9. Reversing the order reverses the
coordinates and direction; these numbers are not millimetres. Only the
connectivity check skips explicitly excluded pads.

The original example's uniform 6/4 thresholds transcribe an earlier register
dump, not a recommended calibration. The working example uses **authored
unequal** thresholds to demonstrate electrode-indexed settings; none was
measured. Every working electrode needs independent calibration through its
unit's shell. All twelve threshold slots remain explicit because the chip
still scans its contiguous E0–E11 prefix; a software exclusion cannot disable
a faulty electrode electrically or clear a global chip fault. Neither pins
example is a qualified live profile.

The opt-in recognizer deliberately separates position and intention. One or
two pads can provide a sampled position; Touch and stationary Sleep require a
continuous qualified three-pad chord. In full-set mode a footprint with an
internal gap over two ranks is ambiguous. In sparse mode the known excluded
slots are skipped, but an inactive working pad inside the footprint makes it
ambiguous. This particular reconstruction stays connected under both rules;
its events and physical coordinates are unchanged. An excluded-only sample
cannot keep a hand held, and a genuine all-working-pads release still ends
the contact after its grace. Once movement evidence exists, the contact cannot become a Sleep
even when it pauses or returns to its starting position. Sleep emits once at
two seconds of observed stationary hold; the reconstructed initial hold can
therefore request Sleep before later slides in the same contact. The final
owner binds the intent to context at contact start: an asleep lamp may instead
receive an explicit Wake. These are separate requested transitions, not a
blind toggle, and Controls does not prove either was applied. Releasing a
slide never creates another Touch or a tap action.

The reconstructed replay expects **54 events**: one Touch, one awake-context
Sleep request, **52 Slide samples**, and no Stroke. All share one contact
identity. It produces 9,975 authored 2 ms ticks and 1,050 output frames through
19,948,000 µs, with zero rejected acquisitions. The separate slow-motion
fixtures exercise Stroke; the recorded fast travels are not relabelled slow.

Reported speed is cumulative sampled travel over at most 200 ms/128 actual
acquisitions. Stroke is a separate authored contact-pattern heuristic: on a
new Slide sample, at least 300 ms since confirmed movement, at least one rank
of cumulative travel, and cumulative travel divided by that moving span no
greater than two ranks/second. The initial stationary hold is excluded. A
pause alone cannot emit Stroke; a pause followed by travel can lower this
estimate. Neither quantity proves continuous physical finger speed.

The tests also author faults, startup-held masks, poll phases, short pulses,
staggered release, reversals, slow movement, and repeated holds. Run from
`lampOS` with:

```sh
cargo test --offline -p lamp-controls --test it strip::
cargo test --offline -p lamp-controls --test it touch_replay::
```

Not shown: real acquisition continuity, threshold noise margins, capacitive
near-hand rejection, physical finger speed or distance, hand-to-Voice latency,
microphone state, acoustic silence, consumer application of an intent, or safe
physical following on the lamp. These fixtures issue no GPIO/I2C operations.
