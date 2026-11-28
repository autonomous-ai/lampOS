# Public EVAL and morning table

Post on the sensor's own issue by **07:00 GMT+7, 28 November 2026**, linking
#8850. Use current v2 when producing new runs; identify older results as v1
and report exactly which current fields/cells remain missing. Do not invent
a conversion that changes labels, provenance or capture clocks. The Reviewer
checks at about 06:30 and refreshes one table before 07:00.

Only aggregate counts, hashes and limitations go on GitHub. Real-person raw
files, words, private paths, participant IDs and trial rows remain local.

```text
EVAL <sensor number/name> — <time GMT+7> — rubric sensor-team-2026-11-27-v2
Status: QUALIFIED / NOT QUALIFIED / invalid or missing evidence
Quality: <correct/planned and % for EACH required class/cell; its exact bar>
Failures: <wrong, unknown, missing, late/incomplete; no discarded primary cases>
Data: <origin; development vs frozen holdout; independent episodes/pairs;
       speaker/session counts; covered distances/lights/noise/face counts;
       exclusions and limits; generated voice/public benchmark caveats>
Latency: <p50/p95, units, exact start→end, clock error/upper bounds;
          first use vs warm; ASR/transport/cold load included or excluded;
          replay host duration separate from null live readiness>
Provenance: <40-hex source/base; executable; full patch if not exact source;
             model; config; scorer revision/binary; manifest/run/report SHA-256>
Retention: keep_all; local inventory/README hashes; consent and raw verification status.
Limits/blockers: <unmeasured lamp behavior, missing cells, source publication,
                  unverified binary/patch, wrong-person/safety outcomes>
No merge / no train. Standalone evidence does not enable this sensor.
```

Report class/cell gates individually; an aggregate average cannot hide a failed
cell. A raw replay score may meet a classification bar while its overall
status is still NOT QUALIFIED. Distinguish public benchmark mAP from exact
speaking-face selection, generated voice identities from real holdout people,
and frame samples from independent utterances. Missing EVALs have no invented
number. Native tests, Clippy and source reviews are not accuracy measurements.
For external development data, report unknown collection consent, geometry,
clock error, light/noise as missing metadata using the v2 adapter path; never
invent these fields to satisfy a physical holdout cell.
For digital caller-voice comparisons, declare `raw_development_pairs`, report
pair counts AND unique probes/references/recording hashes, and disclose shared
sources or augmentations. Those correlated pairs are not independent episodes
and cannot become physical holdout coverage.

For whole C/D, report the simple and plain build identities, played/live
mode, planned counts by scenario, unknown/missing counts and exact audible
latencies. Label **characterization only**. For a one-sensor addition, link
the qualified prerequisite and its hashes; give each B/A target gain, lost
case count, each scenario's score and answer/stop p50/p95 with uncertainty.

The single #8850 table uses this form; the Reviewer fills it from actual
submitted evidence, including the cutoff timestamp and linked EVAL posts:

| Sensor | Number (with denominator/split) | Required bar | Pass or not | Data limits / evidence |
|---|---|---|---|---|
| 1 Hey Lamp | Pending EVAL | Call >=98%; non-call >=95%; >=120 each | Not qualified until checked | Generated vs real, whispers, false wakes and exact timing |
| 2 Words/recipient | Pending EVAL | >=114/120 each | Not qualified until checked | ASR and recipient/gap context, replay readiness |
| 3 Direction | Pending EVAL | >=228/240 joint; >=76/80 faults | Not qualified until checked | Mic alignment, angle vs framing alternative |
| 4 Head/shoulders | Pending EVAL | >=114/120 each; disagreements | Not qualified until checked | Independent shoulders, face visibility, sampling |
| 5 Speaking face | Pending EVAL | >=304/320; >=76/80 each cell; precision/recall >=95% | Not qualified until checked | >=2 visible; face-count/cell/source limits |
| 6 Caller voice | Pending EVAL | >=114/120 same and different | Not qualified until checked | Cross-session pairs, TV, playback, actual admission |
| 7 Interruption | Pending EVAL | >=114/120 each class; true stop <=400 ms | Not qualified until checked | Physical last-audio markers; no natural-completion credit |
| Whole simple C/D | Pending played evidence | Characterization, not addition pass | No promotion | Build/kit/scenario coverage, audible timing |

If a post lacks machine inputs, the table may quote its **reported** raw
number with attribution and an unverified flag; do not claim the scorer ran.
If only a hash is posted, request the lead's local verification rather than
publishing private inputs. Preserve every earlier report and table evidence;
refresh the one public table without rewriting the measured facts.
