# Aging test registration

This is a separate test, prepared before any live score. It keeps the
30-day development fixture and the engine's strictly-over-90-day fading
threshold unchanged. It uses the same fictional owner, retaining every base
month event as an unchanged subsequence in a separate extended stream.

The [scenario](aging-scenario.json) adds sixteen early, closed, two-exchange
conversations: eight ordinary low-importance episodes with detailed disposable
labels and a useful gist, and eight explicitly important long-term controls.
Days 31–120 each have one short two-exchange desk visit. This is sparse activity
relative to the base month's fifty exchanges per day. Eight recent facts are
updated on days 110–117. Only seat activity is supplied; topic and object cues
remain absent. Companion acknowledgements remain deterministic and neutral.
The augmented history is identical for both setups.

The generated stream has 2,856 events and 1,712 owner/visitor utterances.
Daily visits avoid a scheduling ambiguity: all 120 calendar days contain a
maintenance boundary, with seventeen weekly passes after days 7 through 119.
Sleep drains before probes. Calendar dates use the persona's declared zone,
not repeated fixed 24-hour additions. Every target early episode is younger
than 90 days at the day-90 probe and older than 90 days at the day-120 probe.

The 42 fixed questions are:

| Category | Day 90 | Day 120 | Scoring |
|---|---:|---:|---|
| Useful gist from ordinary early episodes | 8 | 8 | Fixed rubric, blinded judge and human audit |
| Important long-term controls | 8 | 8 | Fixed rubric, blinded judge and human audit |
| Recent facts amid old competing detail | 0 | 8 | Exact normalized value |
| Previously forgotten private facts | 0 | 2 | Registered abstention and forbidden-value check |

Every question is human-audited. Gold criteria and sources stay evaluator-only.
Probes use fresh isolated state; day-90 questions do not become main-history
rehearsals for day 120. The shared memory-text cap is 4,096 bytes. The primary
comparison uses ordinary recall; any explicit source-history access is
reported separately because original records and gists survive fading.

Run the full Tiers engine first, then no fading, three fresh runs each.
Both use the same agreed build with nightly topic segmentation enabled. Report
all six attempts, including failures. Compare each setup's mean overall and
per-category score to the full engine's max-minus-min spread for that metric,
using the same retention rule as the month. A safety/privacy necessity can
also retain the mechanism. No thresholds, questions or arms are selected
after seeing scores.

Coverage is a prerequisite to interpreting the comparison. Before scoring,
inspect metadata establishing nonempty accepted detail and gist on the target
low-importance episodes (importance below 0.5), high-importance controls at or
above 0.5, no recall within the preceding 90 days, and maintenance strictly
after the 90-day boundary. Report whether
old eligible detail competes with recent evidence under the fixed cap. A long
script or a day-120 timestamp alone does not prove those conditions. Missing
or unpopulated prerequisites mean **not exercised**, not a reason to drop
fading. Inspection must not change importance or inject ideal model output.

The adapter does not acknowledge delivered recall, so this test cannot claim
to exercise protection of recently delivered memories. The second correctly
identified person is also absent. Both gaps stay beside results. Two correctly
identified people, tested in both directions, are a prerequisite for any
future product-integration proposal.

Generate into a new directory from `lampOS/`:

```sh
cargo run -p lamp-memory-bench -- generate-aging fixtures/memory-bench/development fixtures/memory-bench/aging-scenario.json /tmp/new-memory-aging
cargo run -p lamp-memory-bench -- validate /tmp/new-memory-aging
```

Generation refuses an existing destination. The generated registration pins
the base registration, scenario, event stream, questions and answer key. A
real speech-to-text cache and live judge remain required for scored speech
results. The six aging runs are additional to the 90-run month projection;
the earlier $600 suggested Flash cap does not include them.

The [timestamp-only audit](aging/structure-summary-e8d6dea08.json) is pinned to
the initial aging checkpoint. It finds 120 active days, 29 nightly boundaries
more than 90 days after the first event, and 26 later question turns. These
counts remain applicable because the topic-registration revision changes no
aging event or question bytes. The audit inspected no answer or question
text. It establishes age opportunity only: empty recorded consolidation
cannot establish fading coverage, and a live run still needs the content,
importance and recent-recall prerequisites above.
