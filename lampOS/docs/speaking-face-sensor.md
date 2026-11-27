# Speaking face: offline baseline (#8848)

The [approved sensor 5 plan](https://github.com/autonomous-ai/lampOS/issues/8848#issuecomment-7210818467)
starts with the mouth reading before changing it. The product still runs its
existing speaking-face path. This first step adds an offline journal reader;
it enables no sensor and changes no judgement, voice gate, memory scope or unit.
The approved candidate switch is reserved for the later shadow implementation,
not implemented by this reader. The team lead runs the lamp; the Reviewer owns
the shared sessions and accuracy scorer in #8850.

The subsequent [Light-ASD component candidate](speaking-face-light-asd.md) runs
only in a separate offline bench on the pinned simple-call base. Its switch is
off, and it changes no live sensor or voice behavior. The owner's evening scope
limits speaking-face opportunities to two or more faces, last in rollout order.

## What exists and what this step reuses

Reuse the exact `bus_out::MouthObservation` and `speaking::MouthDiagnostic`
types, including their reason enum, to read the existing
[`--mouth-diagnostics` journal](eyes.md#whether-a-face-is-speaking-lamp_eyes_speakingon-720).
The added deserialization traits do not change their emitted JSON. No copied
mouth algorithm, inference dependency, model, camera capture or service is
introduced. The reader explains saved decisions; it cannot evaluate a threshold
change or a different crop without the classifier's original inputs.

#8791 reports 2 of 88 lines seen speaking. On 27 November the team lead confirmed
that this session's per-line and eyes evidence was not kept: lamp-74ff's journal
rotated, and the voice-set logs did not collect eyes readings. **The exact 2/88
cannot be reproduced from the retained aggregate.** A fresh team-lamp session
must establish a new baseline with its own count, timestamps, binary hashes and
effective switches. Synthetic tests of this reader are not that baseline.

## Inputs and command

Run from the standalone `lampOS/` root, on the builder's machine:

```sh
umask 077
cargo run -q -p lamp-eyes --example speaking_replay -- \
  /private/path/eyes-journal.jsonl /private/path/voice-lines.jsonl \
  > /private/path/speaking-report.json
```

The journal input is JSON Lines from `journalctl -o json`, retaining `MESSAGE`,
`_BOOT_ID` and `_PID`. The reader selects messages beginning exactly
`lamp-eyes: mouth diagnostic `. Unrelated messages are ignored and never echoed.
The lead can keep just these fields and messages in the private export; no
words, audio, image, name or face embedding is needed. Keep the source binary
hashes and the effective speaking/mesh switches beside the export. This command
only reads files; it neither obtains the journal nor accesses a lamp.

The second file is a **numeric line-interval export**, one line per voice request.
It is not the Reviewer's gold-label format. For example, with a synthetic boot:

```json
{"id":1,"boot_id":"0123456789abcdef0123456789abcdef","from_us":1000000,"to_us":2000000,"seen_speaking":null}
```

- `id` is a unique anonymous integer within the export. No words or name fields
  are accepted.
- `boot_id` is the operating-system boot, 32 hexadecimal digits matching the
  journal's `_BOOT_ID`. It is not the eyes publisher's own boot identifier.
- `from_us` and `to_us` delimit the voice line on that boot's monotonic clock,
  with `from_us < to_us`. They must come from an explicit timestamp export;
  do not use a journal's wall time or relabel a different clock.
- `seen_speaking` is the **voice's recorded outcome**, true or false, if it was
  recorded for this line. Missing or null means unreported. Never fill it from
  the mouth diagnostic's `yes`, a nearby known face, a voice-match fallback or a
  human gold label. An absent `seen_speaking_note` is not necessarily false:
  the current voice also omits that note when no face was seen speaking and
  the line kept its person's scope. Keep such an unreported outcome null.

The output includes SHA-256 of each exact input file and keeps every supplied
line, including lines with no matching diagnostics or no reported voice outcome.
`voice_seen`, `voice_not_seen` and `voice_unreported` sum to the line count. They
are counts of supplied observations, not an accuracy score. The reader never
prints a 95% verdict or reconstructs a lost 88-line set.

## What the report can explain

For each line, it reports the largest face's yes/no/unknown counts, returning
reasons, head states, missing mesh inputs, window resets, multi-face looks,
number of distinct eyes PIDs, largest observed same-process gap and largest
logged frame age. These help locate small crops, unread pose, too few looks,
jitter or motion-related rejection. They are observed counts, not independent
root-cause attribution: `reason` is the first returning gate.

Observations are sorted within each operating-system boot. Exact duplicates
from repeated exports count once; conflicting duplicates fail the command.
An eyes process change is reported, and gaps are never measured between PIDs.
Invalid records or limits fail explicitly, rather than dropping unfavorable
data. Limits are 64 KiB per row, one million journal rows, 100,000 unique mouth
observations and 10,000 line-file rows. Errors contain row numbers and fixed
descriptions, never an input message or a person's words.

## Clock and coverage limits

`at_us` is **mouth judgement time**, after detection and inference. This reader
groups it into the line's inclusive `[from_us, to_us]` interval and says so in
`time_basis`; it does not claim capture-time speaker association. In the current
producer, `frame_age_ms` is sampled **before** the look's slow work, so subtracting
it from `at_us` would not recover capture time. It is not capture-to-result
latency. The report does not make that subtraction.

The diagnostic describes only the largest detection, without a track ID. It
cannot identify the speaker in a two-face scene, join an anonymous face across
looks, or recreate the voice's `SpeakingFaces` line-association rules. An absent
diagnostic can mean no valid largest box, disabled diagnostics, loss under log
backpressure or missing export; it never means a silent face or no visible
speaker. A positive diagnostic does not make `seen_speaking` true.

The subsequent shadow candidate needs separate capture-stamped, boot/track-bound
evidence for all candidate faces, privacy/cancellation boundaries, and the
Reviewer's labelled truth. The Reviewer's v1 contract in
`tools/sensors/src/schema.rs` (branch `reviewer/sensor-scorers-8850`, not yet
part of this baseline) keys predictions by `case_id`, with `output`,
`input_end_us`, `ready_us`, `associated_us`, `frame_work_us`, `added_wait_us`,
`held_line` and `safety_violation`. This diagnostic summary is not a prediction
file: none of those identity or timing fields is fabricated from this journal.
Its approved targets remain p95 200 ms combined mouth
work, p95 350 ms capture-to-evidence, at most 5 ms cached line association with no
vision wait, cold readiness within five seconds and useful evidence within two
seconds of the first valid frame. **This offline reader does not measure any of
those device targets.** First use, repeated lines, short lines, two faces and
slow dependencies still need the lead's measurements.

## Verification and what was not shown

The [27 November host check record](verification/speaking-face-8848-20261127/README.md)
lists the exact commands, standalone-source check and Linux limitation.

The example's tests use synthetic numeric observations serialized by the actual
producer type. They cover missing data and boot separation, duplicate and
conflicting exports, process changes, judgement-versus-capture time, old records
without frame age, two faces without inferred identity, invalid input, bounded
reads and errors that do not echo private text. Run:

```sh
cargo test -p lamp-eyes --example speaking_replay
```

No real-person baseline, original 2/88 reproduction, camera-model accuracy,
two-person attribution, on-lamp latency or before/after voice score has been
established by this step. Sensor 5 stays fifth in rollout order, after the head
can frame the person (#8846); none of its product behavior is enabled here.
