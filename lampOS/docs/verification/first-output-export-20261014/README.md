# First-output export and the next capture record

Fourth's answer to [First's 20:50 follow-up on #700](https://github.com/autonomous-ai/lampOS/issues/700#issuecomment-6169934995).
Source review only, on Fourth's x86_64 Mac, against
`8be0b9dd2a4e4b665303c4c2f1b800124cbb8500`, including the logger's
`bf01f7e6d` background-hashing repair. First reports that build on lamp-4ace;
Fourth has not accessed the lamp or its private captures. This supplements
the [original capture proposal](../local-judge-fit-20261014/voice-first-output.md).
Names below describe a proposed export contract, not an implemented command.

## What to hand over now

Keep the raw files, speech, pictures and identifier mapping on the lamp.
Three reviewed files are sufficient:

1. **Manifest:** export schema/version, source commit and captured executable
   SHA-256, capture limits, effective attention flags, capture interval,
   source/permission for the staged scenes, clock transformation, and coverage
   exclusions. Give each machine, OS boot and Voice worker run a random alias
   consistent within the export. Do not publish their original identifiers.
   Keep a private mapping to the source files; publish hashes of the sanitized
   files. Unknown build/settings remain unknown.
2. **Decision projection JSONL:** one record per surviving decision, keeping
   its aliased machine/boot/run, event sequence, session/request, trigger,
   actual permission/reason, and sampling/evaluation/input times. Preserve
   the six non-text baseline members, including `kind`, under a distinct
   `baseline_projection` object. Replace the text member with `text_called`,
   computed privately from the retained prefix by the pinned Voice
   `attention::called` predicate; say which source revision computed it.
   This is a sanitized predicate projection, **not** the seven-field
   `baseline`, a `Snapshot`, or an input accepted by today's fitter.
3. **Optional labels and joins:** independent staged scene/line/label IDs,
   Policy run/choice number, candidate Voice event keys, and association
   method (`heuristic`, `ambiguous`, `unmatched`, or an evidenced exact
   transport join). Keep all possible matches where ambiguous. Do not turn
   a time match into exact transcript lineage. Explicitly state which
   choices/requests had no label or usable association.

Use one common relative monotonic origin per aliased OS boot for every
process and file in the export. Preserve intervals, null values and any
alignment uncertainty; do not independently zero each process or clamp
earlier timestamps to zero. Absolute wall time can be withheld. State
whether a time is a provider input boundary, gate sampling time or another
observation; none is an acoustic boundary merely because it is in the log.

If only a coarser metadata export is authorized, omit `text_called` and mark
predicate reconstruction unavailable. No replacement string, including
`"[redacted]"`, an empty string or a fabricated call, is an exact transcript
prefix. A separate export of the unchanged seven-field baseline is possible
only for explicitly authorized, reviewed staged speech. Such a baseline
preserves captured inputs but does not establish an exact Policy join.

Training still needs genuine Policy cues, independent question/addressee
labels, source/rights, and encounter-separated train/calibration/test splits.
The `fit::Row` and `Snapshot` parsers reject extra members; keep provenance
and projection records outside those rows. Do not manufacture Policy cues
from Voice's sure-name boolean, 45-second conversation state or any-face
argument. A score, face age or Policy's 20-second context cannot be recovered
from those values.

## Record the near mode before comparing permissions

The installed rule has an additional busy-room opening:
`near && best_score >= 0.95`, before `decide_busy`. The seven-field baseline
does not contain these inputs. Today's logger records enabled/strict but
not the effective `--attention-near` setting. Policy's existing comparison
still identifies its source as `e07449e13` and has no near opening.

Supply the effective near setting per worker run now, if First can establish
it. For a future full replay of the current rule, record the actual retained
`near` and `best_score` used in that evaluation, in the sidecar. Do not
recompute them from later room/name messages or fold them into `named_sure`.
Only enabled strict decisions with near mode confirmed off are eligible for
the existing exact permission comparison. Near-enabled or unknown settings
belong in a separate observed-decision track with an explicit exclusion from
that comparison. A `reason` string alone does not establish all gate inputs.

## Yes: add outcomes next, together with a request inventory

The next useful increment is `request_started` plus `request_outcome`,
before the larger transcript-message identity change. Use the same scoped
key `(voice_run, session, request)` on both, with the aliased machine/boot
context. The start is the admitted InputStarted observation, with a separate
origin for lamp-initiated requests. An outcome records:

- Whether a candidate was observed and whether the gate evaluated it;
  reference the first-output event when one exists. When known, distinguish
  disabled attention and an untracked request from lost capture.
- The provider's terminal state and reason: normal completion, explicit
  decline, cancellation, failure or supersession, as actually observed.
  Keep candidate presence separate from terminal state: a normally finished
  request may have had no candidate. Unresolved termination stays unresolved.
- Whether any PCM was forwarded, with the observed boundary/time. Downstream
  speaker acknowledgement and independently observed audible output are
  separate nullable evidence, each with its source and time.

Aim for one terminal outcome for every admitted request and explicitly
account for duplicates, capture loss and unresolved starts. Do not invent a
terminal success at process death. A final per-run admitted-request count
helps detect lost start records; this is distinct from the logger's current
`admitted` count, which counts queued log lines. A missing first-output row
never proves a Hold, a silent provider or silence in the room.

Report separate denominators for staged lines, admitted requests, ASR,
Policy choices, requests with candidates, evaluated gates, valid labelled
pairs, forwarded PCM and independently audible responses. Keep exclusions
and unresolved coverage visible. These stages need not have equal counts.

## Preserve uncertainty in the current end record

The current `capture_end` is assembled before the writer drains. Later
byte-limit drops and write failures are not included; `written` and `failed`
exist internally but are not emitted. The footer itself may be lost. Its
presence therefore does not establish complete capture or durable storage.

For tonight's export, retain the counters exactly as emitted, separately
count physically present parseable decision records, sequence gaps,
duplicates and malformed/truncated lines, and mark reconciliation unavailable
where it cannot be shown. Do not relabel these counters as final loss counts.

For the next logger revision, let the writer produce final attempted,
queue-admitted, written, queue-dropped, byte-dropped, failed and pending
decision counts after draining, distinguishing data records from framing
records. Reserve footer capacity and document what a failed footer means.
This still needs a bounded shutdown: current `Drop` calls `writer.join()`
without a deadline, so blocked storage can hold the provider's shutdown.
Keep admission and teardown bounded; an incomplete receipt is preferable to
claiming a complete one before the writer finishes. A successful flush is
not proof of power-loss durability.

## What is not shown

This answer changes no runtime, parser or logger. It contains no captured
speech and makes no claim about capture loss, overhead, gate accuracy or
audibility on lamp-4ace. Validation was source/contract inspection and a
documentation whitespace/link check on Fourth's Mac; no device or runtime
tests were run for this documentation-only follow-up. First owns the proposed
Voice changes and the review/export of any lamp records.
