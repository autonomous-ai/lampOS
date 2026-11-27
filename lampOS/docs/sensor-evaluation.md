# The shared sensor measuring stick

This is the offline protocol for #8850 / draft #8885. The Reviewer owns the
measuring stick and reads each sensor PR with the team lead; the lead runs
the lamp. The owner's 27 November evening method is: **bench each sensor
alone, bring one passing sensor into one combined judge, bench the whole
again, then consider the next sensor**. No merge and no train. A numerical
pass does not remove that hold or authorize a device change.

Use **version 2**, rubric `sensor-team-2026-11-27-v2`, for the morning EVAL.
The head/shoulder quotas, two-or-more-face cells, interruption sample floors
and whole-bench rules below are this measuring stick's explicit v2 choices
for the assigned overnight evaluation. Earlier approved accuracy and timing
budgets remain. V1 is preserved for reproducibility; its old head classes,
one-face cells and #8673 labels cannot qualify the current jobs.

The simple baseline is the lead's #8892 “Hey Lamp” plus a 20-second renewing
window, before any sensor decides. Its currently published source pin is
`0dbbfc95bbea6ddf397308e1a8ef1933c5d19dcb`; the lead must also pin the actual
binary, complete patch if any, configuration and rendered kit used on the
lamp. Tonight's C (simple) and D (plain 16492) are **characterization**.
Neither replaces a new sensor's A1/B1/B2/A2 experiment.

## Keep every artifact, privately

The owner's latest instruction supersedes the earlier deletion dates:
**keep all data, never delete**. Raw recordings stay on the lead's Mac at
`~/lamp-research/sensor-sessions/`. Retain every original audio/video/frame,
journal, annotation, model input, output, failed run and earlier score.
Each immutable session/run has a README and SHA-256 inventory. Keep consent
for this retention locally. V2 declares `retention: "keep_all"` and rejects
a deletion timestamp; v1's historical field is not a deletion instruction.

Do not publish real people's recordings, frames, words, private paths or
per-person/per-trial records. Public EVALs contain aggregate numbers,
provenance hashes, limits and links. Anonymous IDs and private sidecars join
who spoke, the intended recipient, independent head/shoulder/look labels,
and the actual capture. The scorer needs numeric labels and IDs, not words.

Run `run.sh archive RAW_ROOT NEW_INVENTORY_DIRECTORY` after sealing a session.
It hashes every regular file, refuses symlinks and an output inside the raw
root, and writes a private README and `inventory.json`. It does not move,
copy, upload or delete raw files. `verify-archive RAW_ROOT INVENTORY` checks
the README and every file, including missing/extra files. A changed archive
requires a new inventory; retain the old one. Hashes alone do not guarantee
future storage: the custodian keeps originals and local backups. Run outputs
also contain README and SHA256SUMS; all outputs refuse overwrite.

## One shared collection

Freeze the scorer source/binary, model, thresholds, scripts, eligibility and
split before holdout capture. Freeze adjudicated labels before viewing
predictions. Development people and sessions are disjoint from holdout.
Use at least **six real holdout speakers**, with daylight and lamplight
sessions at least two hours apart; reserve additional development people
(at least eight people overall for the speaking-face collection).

Record mic/camera positions and axes, clock IDs/epochs, clap alignment,
offset/drift/drops and safe seated-face pitch at 600 and 1200 mm. V2 requires
verified source-clock alignment with at most **5 ms** error. Capture clocks
are mapped explicitly; never subtract unrelated server/host monotonic clocks.
The measured error is carried into mapped latency upper bounds. Host elapsed
frame-work/replay times remain their own measurements.

The core is **480 distinct episodes**: 240 calls, 120 lamp-directed followups
and 120 room/phone/near-name lines. English scripts, one/two people, distance,
lighting, noise and intended recipient are varied independently of gaze.
Every sensor's cells are slices of this collection; top-ups keep the same
collection ID and split. Do not fabricate 480 independent clips by reusing
adjacent frames, overlapping intervals or pairing each probe repeatedly.

- Direction: six people × five reachable bearings (-60/-30/0/30/60 degrees)
  × two distances × quiet/background × two fresh utterances = 240 calls.
  Add 80 no-turn fault cases: own playback, overlap, non-directed speech,
  missing/stale/desynchronized input, privacy and stop. A framing-only
  alternative for unsynchronized microphones needs a separate rubric.
- Head and shoulders: at least 120 `facing` and 120 `away`. Label head and
  shoulders separately as lamp/away/unknown. Both toward lamp means facing;
  otherwise an observed away component with both labels known means away.
  Include at least 20 head-toward/shoulders-away and 20 head-away/shoulders-
  toward cases. Unknown components require independently adjudicated gold
  to enter answerable accuracy; otherwise use an unknown fault diagnostic.
  Looking at a phone/screen is no longer a separate primary head class.
- Speaking face: **at least two visible faces** in every applicable primary
  line, 320 answerable lines, four distance × light cells of at least 80.
  Each cell has at least 24 clear speakers in each of the first two visible
  positions and 16 off-camera speakers with silent visible faces. Keep at
  least 192 clear-visible lines overall. Balance sides and face sizes;
  report higher face counts separately. One-face lines cannot inflate this
  job. Ambiguous/occluded cases remain declared diagnostics or fixed primary
  failures, never newly excluded after predictions.
- Words/recipient: 120 lamp-directed and 120 room-directed lines with matched
  wording, 0–3 / 3–10 / 10–20 second gaps from **last actually audible lamp
  output**. Keep outside-window/boundary examples separately labelled.
- Caller voice: 120 same-person cross-session pairs and 120 different-source
  pairs (80 other people, 40 TV), at least 20 over-playback probes in each
  class. Reuse call references and eligible followup/room probes; one pair
  per probe, no split crossing. Frames are not independent followups.
- Interruption: at least **120 each** true interruption, backchannel, side
  talk and noise during lamp playback. Reuse eligible episodes and top up
  rather than pretending the original 480 covers every new cell. Record
  planned playback end, actual last audible sample and observation horizon.
  An unqualified caller-voice feature stays off in the standalone test.

Each sensor also covers first use, slow and disconnected dependencies.
Label performed behavior, not script intent. One episode may have one case
per sensor. Disagreements are adjudicated before predictions. Changing labels,
eligibility or the split creates new bytes/hashes and requires rescoring.
`frozen_unix_s` records protocol freeze before holdout capture, not the later
annotation time. Reviewer checks the collection matrix and blind-label history;
the scorer cannot authenticate a consent claim or observer label by itself.

## Standalone bars and exact clocks

| Sensor | Primary numerical bar | Latency and physical boundary |
|---|---|---|
| 1 Call, #8844/#8907 | >=98% call recall and >=95% non-call rejection; >=120 each | Last required sample → usable result: local p95 <=100 ms, server <=300 ms, 500 ms expiry. Report observed wakes per labelled non-call hour. |
| 2 Words/recipient, #8845 | >=114/120 lamp and >=114/120 room | Final ASR → usable result p95 <=50 ms, expiry 100 ms; speech-end → result separately. Input end equals final ASR. |
| 3 Direction, #8846 | >=228/240 joint angle <=15 degrees AND correct full face within 1 s, one natural turn; >=76/80 no-turn faults | Input → result local 100/server 300 ms p95, expiry 500 ms; call end → observed full face separately. |
| 4 Head and shoulders, #8847 | >=114/120 facing and >=114/120 away, with disagreement quotas above | Source capture → result local 100/server 300 ms p95, expiry 500 ms; opportunity → capture and opportunity → result separately. |
| 5 Speaking face, #8848 | >=304/320 overall; >=76/80 in each applicable cell; >=95% clear-visible recall and face-pick precision | Frame capture → publication p95 <=350 ms; frame work p95 <=200 ms; cached line-close association <=5 ms; age <=500 ms. |
| 6 Caller voice, #8849 | >=114/120 same and >=114/120 different | Clip submission → result p95 <=500 ms; no admission wait. Report readiness by line-end+100 ms separately from actual first admission. |
| 7 Interruption, #8923 | >=114/120 in EACH of interrupt/backchannel/side talk/noise | True interrupt: acoustic onset → last audible lamp sample <=400 ms including clock error. Negatives: no stop/restart through planned playback end and declared observation horizon. Detector result retains local 100/server 300 ms p95. |

Rates use every planned primary case. Missing predictions, unknown outputs,
late/incomplete evidence and unknown primary truth are failures. Safe unknown
fault abstention is reported separately and earns no primary accuracy credit.
A correct label with missing physical evidence is not a qualified success.
A natural playback completion is not an interruption stop. No wait/hold/safety
violation is allowed. Missing timing is null; never replace it with zero.

`ready_us` is the time usable evidence reached its consumer on the capture
clock. Replay has **null readiness** and optional `replay_elapsed_us` for
host invocation→result duration. It cannot supply fabricated live framing,
association, admission or audible-stop timestamps. Cold initialization, ASR,
transport and model-only timings name their actual boundaries and exclusions.

Run evidence (`lamp_live` / `replay` / `synthetic`) is separate from session
origin (`real_people` / `generated` / `public` / `invented`). Qualification
needs the consented real-person collection, including its actual TV/noise
exposures, not generated identities or public benchmark subjects. Generated
and public results remain useful standalone development numbers, with limits.
Training permission is source-specific: do not extend a sensor's evaluation-
only permission to another dataset or undo #8907's authorized generation work.

Public/generated/invented **development** inputs may leave collection consent,
clock error, distance and bearing null, and light/noise `unknown`, when those
facts were never measured. The report counts missing metadata explicitly;
no fabricated distance, consent or clock value is required. This path does
not fill any real-person qualification cell. Real-person sessions, holdout
requirements and every primary quality/timing bar remain unchanged. See the
v2 adapter guide for the exact limited encoding.

Reports retain raw classification by development/holdout, class and origin,
physical/timely counts, confusion, missing coverage, speaker/session/condition
slices, face-count and head/shoulder disagreement slices, p50/p95 and a
session-cluster bootstrap interval. Raw label agreement is never a live pass.
An interval from two sessions does not establish population accuracy.

## Bench the whole, one addition at a time

Use `compare-whole`, not the historical v1 `compare`. Freeze actual rendered
assets and a new typed case-vector hash before predictions; the old 666-line
#8673 set is not silently relabelled. Include **played and live** modes, at
least ten fixed cases in every scenario per mode, six real live speakers and
two sessions. The overlap scenario needs ten in each of four classes, so
there are at least **90 cases per mode per leg**, before useful top-ups.

| Scenario | Required evidence |
|---|---|
| Alone | Greeting/call and ordinary requests, including a fresh call outside the window; first and repeated use. |
| Noisy room | Same tasks with declared background level/source, correct audible answers and negative exposures. |
| Two people | Both people as caller, changed speaker, out-of-view cases and wrong-person/side-talk checks. |
| Talking over the lamp | >=10 each real interruption, backchannel, side talk and noise; actual playback and acoustic stop evidence. |
| Followups in 20 s | Early 0–3, middle 3–10, late 10–20, inside 19.5–20, outside 20–20.5, later outside and explicitly closed-window slices. |
| Side talk | Room-directed lines inside and outside the window; independently verified stimulus and no reply. |

Boundary labels require the entire measured clock-error interval to fit the
band. Use points safely either side; an uncertain exact 20 s is unknown. An
ignore case needs at least 10 s of independently observed silence after the
line, with no actual reply onset. Continue cases need proof of uninterrupted
playback through planned completion. Never infer “heard” from transcript
presence or “stopped” from a cancellation flag.

For a one-sensor addition, A1/A2 are the pinned accepted baseline and B1/B2
add exactly the named, standalone-qualified sensor to **one combined judge**.
Keep all other settings/assets fixed, and run A1,B1,B2,A2 in that recorded
order for each mode. Pin source revision, actual binary, full uncommitted
patch if any, config and model manifest per build; each leg has its own raw
inventory and measured source clock. The added component's model/config
must match the qualified standalone report. The CLI recomputes that report
from its original manifest/run with the frozen scorer before accepting it.

“Improves its target and drops nothing” means, separately for B1 vs A1 and
B2 vs A2 in **both modes**:

1. At least **one additional correct case** in the predeclared target scenario;
   report its fraction and percentage-point gain. No rounding into a gain.
2. **Zero previously correct cases lost**. Every scenario, speaker, session,
   class and window slice keeps or improves accuracy on fixed denominators.
   Missing/unknown/timing coverage cannot worsen; required evidence is complete.
3. Answer (acoustic line end→first audible output) and stop (acoustic onset→
   last audible output) p50 AND p95 do not increase. Compare full samples and
   matched cases. Candidate upper clock bounds must be <= baseline lower
   bounds; equal measurements inside uncertainty do not establish no loss.
4. Each overlap class meets >=95%; real interruption p95 is <=400 ms with
   error included. With only ten cases/class this requires 10/10. No safety
   violation, changed assets/config, missing leg or unqualified prerequisite.

C/D is `baseline_characterization`, always report-only (`passed:false`). Its
native whole scores and exact latencies can be reported without claiming an
addition gate. A partial run still reports its planned denominators and gaps.
Do not make up a frozen kit or source-clock measurement to pass validation.

After a numerical pass, the lead and Reviewer verify raw provenance, limits
and the combined-judge behavior. Only a separately authorized integration may
bring that single sensor over. Re-bench the whole before considering another;
any regression keeps that addition out. Current no-merge/no-train hold remains.

## Commands and morning table

All wrapper paths must be absolute. The standalone Rust workspace is offline
and outside product dependencies; it opens no camera, microphone or socket.

```sh
tools/sensors/run.sh validate MANIFEST
tools/sensors/score-call.sh MANIFEST RUN NEW_OUT
tools/sensors/score-addressed.sh MANIFEST RUN NEW_OUT
tools/sensors/score-direction.sh MANIFEST RUN NEW_OUT
tools/sensors/score-head.sh MANIFEST RUN NEW_OUT
tools/sensors/score-speaking-face.sh MANIFEST RUN NEW_OUT
tools/sensors/score-voice-match.sh MANIFEST RUN NEW_OUT
tools/sensors/score-barge-in.sh MANIFEST RUN NEW_OUT
tools/sensors/run.sh whole-cases-hash CASES_JSON_ARRAY
tools/sensors/compare-whole.sh COMPARISON SENSOR_MANIFEST SENSOR_RUN SENSOR_REPORT NEW_OUT
tools/sensors/compare-whole.sh CD_COMPARISON - - - NEW_OUT
tools/sensors/run.sh archive RAW_ROOT NEW_INVENTORY_DIRECTORY
tools/sensors/run.sh verify-archive RAW_ROOT INVENTORY_JSON
```

Exit 0: requested numeric gate passes; exit 1: complete report says not-qualified
or characterization/hold; exit 2: invalid input/I/O, no valid score. Every score
pins exact input and executable hashes. Use a frozen built executable rather
than rebuilding mid-collection. Source pins, patch and executable hashes are
distinct. `build_sha` is 40-hex Git; binary/full patch/model/config are 64-hex
SHA-256. Unknown JSON fields are rejected. No private text in numeric inputs.

See [the EVAL template](../tools/sensors/eval-template.md), [v2 adapter guide](../tools/sensors/adapter-v2.md)
and executable contracts [schema.rs](../tools/sensors/src/schema.rs),
[score.rs](../tools/sensors/src/score.rs), [whole.rs](../tools/sensors/src/whole.rs).
Around 06:30 on 28 November the Reviewer checks each EVAL and posts **one table
on #8850**, refreshed before 07:00: sensor, measured number with denominator,
bar, pass/not, data limits. Link the submitted EVAL and report hash. Missing
EVALs say not-qualified/missing; development scores keep their split and do
not become holdout passes. Record cutoff time and missing evidence explicitly.

Tool fixtures are invented regression tests. They demonstrate the measuring
code's behavior, not sensor quality, physical motion safety or lamp latency.
Local/ARM test results, team EVAL measurements and remaining limits are reported
separately. No device measurement is supplied by this tool implementation.
