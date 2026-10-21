# The six resumed yield gaps, 22 October 2026

Timing-only evidence for Third's [request on #2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6321836281).
These are subsets of the first builder's two existing lamp-4ace exports,
not new measurements. Fourth has no lamp and made no provider request.
The aggregate recount is in [#2684](https://github.com/autonomous-ai/lampOS/pull/2684).

All six cases have **a `frame_sequence` increase of 75 between the recorded
yield and resume decisions**, taking 747.111–752.458 ms by the coordinator's
timestamps. The accepted zero PCM lasts 740 or 750 ms at 24 kHz. These
are different boundaries; neither is measured acoustic silence.

| Arm | Turn | Owner generation | First sample epoch / frame | Accepted zero frames | Accepted zero ms | Yield to resume ms | Selected rows |
|---|---:|---:|---|---:|---:|---:|---:|
| `ab1840base` | 28 | 57 | 1 / 6810240 | 17760 | 740 | 751.240 | 80 |
| `ab1840base` | 44 | 89 | 1 / 16826400 | 17760 | 740 | 747.111 | 8 |
| `ab1840base` | 49 | 99 | 1 / 19236480 | 17760 | 740 | 748.287 | 35 |
| `ab1840base` | 59 | 119 | 1 / 22872240 | 18000 | 750 | 752.458 | 36 |
| `ab1913po350` | 34 | 69 | 1 / 9338160 | 17760 | 740 | 750.184 | 8 |
| `ab1913po350` | 51 | 103 | 1 / 20201280 | 18000 | 750 | 749.858 | 34 |

## Files and selection

[manifest.json](manifest.json) preserves each complete nested owner
(boot bytes, turn and generation), gap start timestamp, first sample
cursor, source filename and SHA-256, selected source line numbers,
output SHA-256, row counts and selection boundaries. The selected rows
contain no separate provider request ID; none was synthesized from a
clock match. Five `local_endpoint` rows retain their endpoint classifier's
worker/request identity: worker 1, requests 29/40/48/56 for the first arm
and request 51 for the second arm's turn 51. Turn 34 used the recorded
short silence fallback and has no verdict request. These classifier IDs
are distinct from provider request IDs: the
[endpoint wire contract](../../../crates/turn-end-wire/src/lib.rs) defines
worker as the classifier's process incarnation and request as its request
number within that classifier.

- [ab1840base, turn 28](ab1840base-turn-28.jsonl)
- [ab1840base, turn 44](ab1840base-turn-44.jsonl)
- [ab1840base, turn 49](ab1840base-turn-49.jsonl)
- [ab1840base, turn 59](ab1840base-turn-59.jsonl)
- [ab1913po350, turn 34](ab1913po350-turn-34.jsonl)
- [ab1913po350, turn 51](ab1913po350-turn-51.jsonl)

The six JSONL files contain **201 unmodified source lines** in their
original order. No field is removed, added or rewritten within a selected
line. Each file includes:

1. The reply's `input_admitted`, `local_endpoint` and `speaker_first_write`,
   including its complete endpoint verdict and `start_gate`, even when
   those precede the gap window.
2. The matching owner's `duplex_yield` and `duplex_resume` and the unique
   matching `playback_gap` start/resume pair.
3. All same-turn `provider_audio_enqueued` rows from one second before
   the yield through 100 ms after the resume, inclusive, plus the nearest
   same-turn enqueue before that window. There are 153 in-window rows
   and six preceding context rows. The two windows with no enqueue still
   retain that preceding row.

Source/worker/coordinator timestamps, queue/held/pending fields, numeric
sequence IDs, gap cursors and zero-frame counts remain as recorded.
The second arm's turn 34 endpoint also retains its 200 coarse level/score
diagnostics in `last_blocks`; these are not PCM or the gap's missing
capture history. These endpoint context rows precede their yields by
3.974–52.187 seconds.
The `start_gate` fields are not reconstructed: each of the first arm's
four replies records a 100000 us horizon; each of the second
arm's two replies records 350000 us. Those observations do not establish
the complete build, policy or environment flags.

## Missing evidence

Neither supplied timing export contains `duplex_hearing` or
`duplex_yield_end` rows, stop-word result rows, capture-discontinuity
rows, or missing/dropped markers. Their companion timing journals also
retain none of the requested hearing, stop-word or discontinuity detail.
This is missing evidence, **not zero observed failures**. Both source
files' complete record-kind counts are in the manifest.

Therefore these slices cannot supply the requested complete hearing
history from one second before yield through the first post-resume
block. The chosen 100 ms tail is an explicit selection bound, not proof
that the first post-resume capture block is present. The exact build,
policy and complete flags are also absent from all four supplied files.
The first builder must recover those from the original sessions or
deployment record before Third can distinguish intended stillness from
capture delivery, a decision delay, a queue delay or a speaker delay.
No yield/resume policy change follows from this subset alone.

## Verification

The original exports live under
`~/lamp-voice-data/sessions/gaps-1021/` on Fourth's development Mac:

| Arm | Source | Records | SHA-256 |
|---|---|---:|---|
| `ab1840base` | `7d22bb8a22634c90a316b33da953842c.timing.jsonl` | 12689 | `b533ba764656fc0793afcdc99222bcad79993d9364553418cb5b18fa60f05cc1` |
| `ab1913po350` | `3afa5034d9e34e1a9b7a0edc27cb64d5.timing.jsonl` | 10694 | `9bf20e8986f9f1b2532f2ae325298d553428bf123573619f5e7b03bf16816708` |

The scratch extraction program is
`/private/tmp/fourth-yield-gap-timings-checks/extract.py`, outside the
repository. It verifies the two whole-input hashes, selects the six
unique resume cases, matches full owners and gap cursors, checks frame
arithmetic and the 75-count sequence differences, and rejects unreviewed record
fields or string values before writing the original bytes. It exports
only timing, numeric identifiers and fixed schema/diagnostic strings;
there is no transcript, audio, credential or free-form journal text.

Reproduction requires the exact two inputs above and this procedure:
find the four `duplex_resume` rows in the first export and two in the
second; for each, select its last earlier same-owner yield, the one gap
starting between those decisions and resuming within 100 ms afterwards,
the matching start receipt, admission, endpoint and first write, and the enqueue
rows described above. Concatenate the original source lines in source
order. The manifest gives every expected line number and file hash.
These files are evidence only; no runtime, test, default or dependency
changes, and no lamp comparison is claimed.
