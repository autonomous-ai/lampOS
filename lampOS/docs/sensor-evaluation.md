# The shared sensor measuring stick

Implemented for #8850, following the team lead's 27 November approval and
the approved plans in #8844–#8849. These are offline tools and a recording
protocol, not permission to enable a sensor or a claim of device accuracy.
The Reviewer owns labels/scoring and reads each sensor PR with the lead.
The lead alone records and runs the team lamp. Every candidate ships off;
an unqualified sensor cannot hold a line or delay admission.

The product owner's 27 November 16:2x direction permits publishing our own
branches, draft PRs and reviews, but **no merge and no train entry**. Show each
sensor's own quality first. The voice baseline is being reset to **"Hey Lamp"
plus a 20-second renewing window, with no sensor deciding**. After that base
is reviewed and pinned by build and configuration, evaluate one sensor's
addition at a time. Earlier stable results and source READ-OKs do not authorize
integration or substitute for this new comparison.

## Recording once for all six sensors

Raw recordings stay **only on the lead's Mac**, under
`~/lamp-research/sensor-sessions/`. Each session has explicit consent and
a deletion time. Do not put recordings, frames, transcripts, names, or
private paths in Git, issues or journals. Use anonymous participant IDs.
The scoring manifest contains numeric/anonymous annotations and script IDs;
the script text, ASR words and conversation context live in a private
sidecar keyed by trial ID. Sensor adapters may read that sidecar locally;
the scorer never needs or exports its text.

1. Before collection, freeze this scorer's commit/binary, the model and
   threshold, the script, the eligibility rules and the participant split.
   Calibration people and sessions are disjoint from evaluation. Use at
   least six evaluation speakers; reserve additional development people
   (at least eight people altogether for the speaking-face plan).
2. At the first lamp session, record mic models, positions and camera axes;
   the mic1/mic2 clap train, offset/drift/drops; camera and playback clock
   alignment; and safe seated-face pitch at 0.6 and 1.2 m. Annotate every
   monotonic clock/eyes epoch. No acoustic bearing is qualified before
   Tests has shown that the capture pair supports it. A framing-only
   replacement needs a new approved rubric, not a passing angle score.
3. Collect the shared pool over daylight and lamplight sessions at least
   two hours apart. The core pool is **480 distinct episodes**: C001–C240
   calls, F001–F120 lamp-directed follow-ups, R001–R120 room/phone/near-name
   lines. English scripts, one or two people, with intended recipient,
   head target and look target independently varied. A person can address
   the lamp while looking at a screen.
4. C001–C240 fill the direction grid: six people × five bearings
   (−60, −30, 0, +30, +60 degrees) × two distances (600/1200 mm) ×
   quiet/background × two fresh utterances. Validate reach before freezing
   positions. Keep unreachable cases as documented limitations, not silently
   removed failures. Alternate repetitions across sessions and lighting.
5. The mouth's 320 cases are a **slice of those same episodes**: eight
   cells (one/two visible faces × both distances × both lights), 40 each:
   24 clear visible speakers, eight off-camera speakers with silent visible
   faces, eight ambiguous/occluded/overlap cases. Use 192 suitable call
   episodes for clear speakers and 128 suitable F/R episodes for the other
   two classes. Balance both speaking faces, sides and face sizes. The
   remaining episodes still contribute to other sensors. Top up cells
   that cannot be filled without changing the scene's truth.
6. Head direction uses at least 80 clear episodes for each of lamp, screen
   and phone; no-face/occlusion cases are separate. Follow-up classification
   uses 120 addressed and 120 room lines within 20 seconds of the previous
   lamp playback end. Keep window-edge cases separately labelled. Voice
   match uses 120 cross-session same-person pairs and 120 other-source
   pairs (80 other people, 40 TV), with 40 over-playback lines balanced
   between classes. Reuse C references and F/R probes where eligible;
   every probe has at most one pair, and neither side crosses the split.
7. Add/top up the 80 direction negative/fault trials: own playback,
   overlapping callers, non-directed lines, missing/stale/desynchronized
   input, privacy and stop. Include first use, slow and disconnected
   dependencies for every sensor. Fault rows are distinct from the primary
   accuracy denominator and never erase a planned primary miss.
8. Label actual performed behavior, not merely the planned script. Independently
   check/adjudicate labels before seeing predictions. One episode can have
   one case per sensor; adjacent frames or reused pair combinations cannot
   increase its denominator. Non-overlapping capture intervals identify
   distinct episodes. Record simultaneous speakers as one overlap episode.

Keep immutable manifest bytes after adjudication and before scoring. Changing
any label, split or eligibility creates a new manifest SHA and requires
rescoring both sides. The manifest's `frozen_unix_s` is the **protocol**
freeze before holdout capture, not a claim that recordings already existed.

## Run the tools

The standalone Rust package is `tools/sensors/`, outside the product workspace
and dependency graph. It opens no microphone, camera, socket or network
connection. Its wrappers build/run with Cargo's offline, locked mode.
Prepare dependencies once with `cargo fetch --locked` on the development
Mac if necessary; the recording session then needs no package download.

From `lampOS/`:

```sh
tools/sensors/run.sh validate /private/path/manifest.json
tools/sensors/score-call.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/score-addressed.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/score-direction.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/score-head.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/score-speaking-face.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/score-voice-match.sh MANIFEST RUN NEW_OUTPUT_DIRECTORY
tools/sensors/compare-voice.sh COMPARISON NEW_OUTPUT_DIRECTORY
```

All paths after a wrapper must be absolute (the wrapper enters its own
directory). Outputs are `report.json` and, for a sensor run, `trials.tsv`.
An existing output directory is refused. Exit 0 means the requested gate
passes, 1 means the report says NOT QUALIFIED/HOLD, and 2 means invalid input
or an I/O failure. `validate` only checks the schema; it is never qualification.
Every report pins the input hashes and the actual scorer executable hash.

The exact typed v1 contract is [schema.rs](../tools/sensors/src/schema.rs).
Unknown JSON fields, duplicate IDs, reused capture intervals, mismatched
hashes, wrong splits, missing consent and backward clocks are errors.
Optional fields may be omitted or null; do not replace missing timing with
zero. Strings used as IDs contain only letters/digits, `_`, `-`, `.`, `:`.
Use the JSON examples in [the tool README](../tools/sensors/README.md).

### Manifest and prediction boundaries

- `sessions`: ID, development/holdout split, Unix start/consent deletion
  times, capture `clock_id`, measured `sync_error_us`, recording SHA-256.
- `trials`: ID/session/speaker/source (`person`, `tv`, `lamp`), monotonic
  `start_us`/`end_us`, optional `call_end_us`, last lamp playback end,
  recipient, script ID, distinct `look_target`/`head_target`, bearing,
  distance, light/noise, temporary `visible_faces`, labelled
  `subject_face_id`/`subject_visible`, `eyes_epoch`, overlap/first-use and
  condition (`normal`, `slow`, `disconnected`). Track IDs are only meaningful
  within their eyes epoch. Visible faces are listed left to right.
- `cases`: ID, sensor (`call`, `addressed`, `direction`, `head`,
  `speaking_face`, `voice_match`), trial ID, expected result, `fault`,
  optional reference trial for voice pairs, and the head sensor's declared
  `opportunity_us` inside the capture interval.
- `run`: version 1, exact manifest SHA, sensor, 40-digit build SHA,
  model and threshold/config SHA-256, `evidence` (`lamp_live`, `replay`, `synthetic`),
  `location` (`local`, `server`) and predictions keyed by case ID.
  Optional `producer_binary_sha256` and `source_patch_sha256` identify the
  producer executable and complete uncommitted diff; a patch hash requires
  the executable hash, and `build_sha` then names the base Git revision.
- Every prediction includes the session's `clock_id`, `output`, source `input_end_us`, `ready_us`,
  `added_wait_us`, `held_line`, `safety_violation`. It also carries the
  fields its job needs: final ASR time; eyes epoch/subject/reason; cached
  association/frame work; measured direction/full-face time/turn count;
  wake count. `reason` is an anonymous token such as `no_face`, `occluded`,
  `wrong_subject`, `stale`, `timeout`. A missing row remains a miss.

For replay, `ready_us` must be null/absent. A separate optional
`replay_elapsed_us` measures host processing duration; never add it to a
recorded timestamp. Replay cannot supply live association, full-face or
admission timestamps. Raw label agreement is reported separately from the
ordinary timing/behaviour counts, whose missing readiness remains a miss.
See [adapter examples](../tools/sensors/adapter-examples.md) for exact addressed,
voice-match and private request-to-case joins. These fixtures are invented.

Do not set `lamp_live` for offline replay. Reports cannot establish provenance
from a label alone: the lead/reviewer must match hashes to the actual recording,
model, settings and running binary. A schema-valid synthetic fixture is not a
measurement. Helpers never change product switches or grant release approval.

### Gates and timing

| Sensor | Accuracy gates | Timing boundary |
|---|---|---|
| Call | ≥98% recall and ≥95% non-call rejection, ≥120 each | Last required input → usable result: local p95 ≤100 ms, server ≤300 ms; 500 ms expiry |
| Addressed | ≥95% each class, ≥120 each in the 20 s window | Final ASR → result p95 ≤50 ms, expiry 100 ms; speech-end → result is reported separately |
| Direction | ≥240 calls; ≥95% jointly ≤15° and correct full face within 1 s of call end, one natural turn; ≥80 faults with ≥95% no turn | Input → result as above, plus call-end → full face; each grid cell ≥12 calls |
| Head | ≥95% recall separately, ≥80 each for lamp/screen/phone | Source capture → result p95 ≤100 ms local /300 ms server, expiry 500 ms; sampling and opportunity → result separately |
| Speaking face | ≥304/320 overall; ≥38/40 in each cell; ≥95% visible-speaker recall and face-pick precision | Frame capture → publication p95 ≤350 ms, frame work ≤200 ms, cached line-close association ≤5 ms; evidence age ≤500 ms |
| Voice match | ≥114/120 same and ≥114/120 different | Clip submission → result p95 ≤500 ms; count separately how many were ready by line end +100 ms |

The mouth and voice-print boundaries reflect their specifically approved plans,
not an exemption for blocking voice. Work during a line can finish before its
end. `speech_end_to_result_us` is the nonnegative extra time after the end;
it is zero for evidence already ready. A voice print arriving after admission
can only help renewal; the scorer does not turn it into an admission vote.
`within_admission_budget` counts readiness by line end +100 ms; it is not
proof that the result preceded the actual admission event. Optional
`first_admission_us` produces a separate `ready_before_first_admission`
comparison (null when unmeasured). The runtime must still check actual
admission/cancellation before using it.
For addressed speech, `input_end_us` must equal `asr_final_us`.

Direction's `framed_speaker_id`, `face_in_frame_us` and `natural_motion` are
observer annotations of the correct full face and natural one-turn motion,
not a detector's own claim. Head predictions must name the annotated subject
and correct eyes epoch; a correct class for another face is a miss. Speaking
face returns a temporary visible track ID, `no_visible`, or `unknown`.
Unknown is correct only when the independently labelled expected result is
unknown. Missing detections on visible people, late outputs and unanswered
trials stay in their denominators. Every supplied hold/wait or safety violation
fails qualification. Six speakers, separated sessions, the shared 480 episodes,
and first-use/slow/disconnected coverage are also required.

Reports include per-row outcomes, confusion counts, missing/late coverage,
speaker/session/cell/condition slices, wrong-person picks, p50/p95 timing and
a deterministic 2,000-resample session-cluster bootstrap interval (unavailable
with fewer than two sessions). This interval does not prove population accuracy.
Wake rate is per hour of **labelled non-call exposure**, not unobserved wall
time; every such prediction needs an observed `wake_count`. No measurement is
invented for missing data. Clock uncertainty, actual microphone feasibility,
cell balance/side swaps and physical safety still need the lead's evidence read.

## The before/after voice gate

The reset baseline and its expected voice outcomes have not yet been frozen
for this tool. The v1 kit below records the previously reviewed rules and
remains useful for historical comparisons and comparator tests. Its
`passed` result is **not a voice gate for the reset baseline**. Before any
switch-on comparison, the lead and Reviewer must review the reset policy's
case labels, freeze the matching kit/scorer revision and rendered assets,
and pin the baseline build/configuration. Do not silently relabel v1 or set
`kit_reviewed` merely because the earlier kit was reviewed.

[played-kit-v1.json](../tools/sensors/played-kit-v1.json) pins #8673 at
`f2d57a11a0754c9d2649f3c903e6679ffc241275`, after both readers examined its
HOLD fixes. The default set is 58 groups, 421 scenes, 689 played lines,
**666 scored A/Q/S lines**. `handover` is excluded; `october-exact` is removed;
the 26 November call rule is the default. The frozen case manifest stores
only anonymous scene IDs, groups and expected outcomes. The comparator refuses
a subset, altered labels or substituted groups. Unscored setup lines still play.

Before A1 the lead renders and hashes the actual voices and noise beds on
the recording Mac. Keep macOS voice/rate versions and WAV bytes fixed across
all legs. Use mac voices for the cue-dependent groups unless equivalently
rendered and separately reviewed. `kit_sha256` identifies that full local
source/assets manifest, not just a source commit. The code pin is ready;
no rendered audio bundle is supplied here.

The comparison JSON contains build SHAs, the one `changed_switch`, approved
kit hash, `kit_reviewed`, scene-manifest hash, unchanged-settings hash, frozen
cases and eight runs: A1/B1/B2/A2 for played, then the same four for live
(interleaving modes is allowed, changing each mode's order is not). For the
future reset-baseline gate, A1/A2 use the pinned "Hey Lamp" plus 20-second
renewing window with no sensor deciding; B1/B2 add only the separately
qualified sensor under review. Live has at least ten calls, ten follow-ups,
ten room lines and ten stops,
with matched participants, positions and levels.

Each result line records its case ID, observed A/Q/S/missing, whether heard,
speech end, first actual audible output, interruption onset and last actual
output. Export these from synchronized capture/playback evidence. Do **not**
rename a decision-to-first-write interval in a legacy TSV as acoustic latency.
The existing `voiceset.py` card remains useful for native counts; its exclusion
of UNHEARD Q rows cannot improve the fixed planned denominator here.

Each group in B1 is compared with A1 and each in B2 with A2. Correct count
cannot fall, unheard/missing cannot rise, and answer/stop p50/p95 cannot rise.
There is no averaging a bad group or leg into a better total. Missing legs,
missing lines/timing, changed assets/settings, unreviewed kits and safety
violations HOLD. A passing report still needs both readers and lead-run
on-body sensor evidence, and never lifts the current no-merge/no-train hold.
Once integration is authorized, add one sensor at a time; any drop rolls it
back.

## Validation and limits

The tool's Rust tests exercise all six passing synthetic fixtures and the
failure boundaries: missing/duplicate rows, 98% call recall, split leakage,
consent, capture reuse, cell failures, wrong subjects/epochs, ASR timing,
late voice prints, A/B/B/A ordering, unseen lines, asset mismatch, latency
regressions, and CLI output/exit behavior. Build/format/clippy/test commands
are in the tool README. No actual voice, camera, motion, sampling delay,
microphone synchronization or end-to-end device latency has been measured
by this implementation. Those remain the lead's recording session.
