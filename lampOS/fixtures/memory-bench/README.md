# Memory benchmark fixtures

Everything here is fictional. The development persona has 40 facts, twelve
traits, ten changed facts, two explicit forget requests, six appointments,
visitors and decoys. The generator emits thirty days with fifty utterances
in eight conversations across four desk visits per day (2,220 events total). Its 200 questions include twelve
three-turn appointment probes. No arm has been scored against this material.

Registration version 2 adds three day-9 wrong-person probes, replacing three
visitor disclosure probes while retaining 200 questions. On day 8 a fabricated
second resident supplies three statements while the observed label defaults
to owner. True speaker identity exists only in the evaluator key. The hard
gate is zero owner facts or owner-model lines learned from those records;
missing inspection evidence cannot establish a pass. Version 1 remains in
the repository history; no scored run preceded this change.

Registration version 3 revises only timing, boundary events and the resulting
question eligibility lists before any score: 2–12 exchanges per conversation,
3–30 seconds apart, with long gaps and observed arrivals/departures. Persona,
owner utterance content/IDs, question wording/scoring and answer keys stay unchanged.
Empty companion replies become a deterministic rotation of “Got it.”, “Okay.”
and “I hear you.”, shared by all setups. They add no facts or promises and
cost no model calls. This month does not test real lamp replies or promises.
The prior nine-minute spacing falsely split every utterance into a product
conversation. Consolidation remains 30 nightly and four weekly passes.
The facts and ahead removal names now match the adapter manifest, with no
change in component meaning or order. Prior plumbing attempts remain retained.

Files are deliberately separated:

- `persona.json`: evaluator input to the generator; never passed to an arm.
- `events.jsonl`: the clean observed-event script. Only this stream may reach
  an adapter, after a real frozen speech-to-text cache replaces its heard
  text for spoken-input evaluation. Unknown confidence is null, not 1.0.
- `answer-key.jsonl`: evaluator-only source links and true speaker labels.
- `questions.jsonl`: evaluator-only questions, answers/rubrics and schedules.
  An adapter receives only each question's query, not the gold fields.
- `registration.json`: content hashes, arm order, limits and pending reviews.

From `lampOS/`, reproduce into a new directory:

```sh
cargo run -p lamp-memory-bench -- generate fixtures/memory-bench/development/persona.json /tmp/new-memory-month
cargo run -p lamp-memory-bench -- validate /tmp/new-memory-month
```

Generation refuses an existing output directory. Compare the output files
byte for byte before changing a registered fixture. The development generator
places corrections and deletions on days 8–19, between the day-7 original-value
questions and the day-20-and-later updated-value questions; it rejects
incompatible dates and colliding scripted events. Its persona declares an
IANA timezone; it does not simulate dates by adding fixed 24-hour intervals.

`eligible_evidence_ids` allows citations to the ingested prefix ending at the
question's `after_record`, not later records before its query timestamp;
`evidence_ids` identifies the specific rubric's source material. These are
not the same: a correct follow-up may legitimately cite another past plan.
The appointment probes name the plan in their spoken question; they receive
no supplied topic cue.

Exact scorers compare the whole normalized answer, dates use a registered
one-day tolerance, and abstention requires a complete registered form.
A rubric's `forbidden` list is only for literal confidential disclosures,
including beside punctuation. Semantic prohibitions and negation are rubric
criteria, so "do not interrupt the call" is not itself penalized. Rubric
scores remain provisional pending a blinded judge and human audit.

The held-back folder is intentionally empty. Human review of this development
month, real TTS/STT cache generation, held-back authoring, and all live arm
scores remain outstanding. Recorded-provider unit tests do not satisfy those
steps. See `docs/memory-bench.md` for the arm contract and acceptance gates.

## Recorded month plumbing

`recorded-answer.json` is a deliberately synthetic fixed abstention, with
synthetic usage. It is not a captured model answer and carries no accuracy
information. The `plumbing` command passes it through the exact-request
recorded provider and shared budget/audit client for every answer turn. It
never invokes a scorer. `plumbing-card` verifies the append-only run ledgers
and formats counts by question kind with `scores: null`.

```sh
lamp-memory-bench plumbing fixtures/memory-bench/development /tmp/new-plumbing-run none FULL_SOURCE_REVISION FULL_REGISTRATION_REVISION fixtures/memory-bench/recorded-answer.json
lamp-memory-bench plumbing-card /tmp/new-plumbing-run
```

The in-process reference adapter is launched as `arm FULL_SOURCE_REVISION`
inside the supervised child process. `none`, `full_history` and
`lampos_today_partial` are available. Every run uses fresh private state and
records the actual executable hash alongside the caller-pinned source
revision. Run only the executable built from that clean revision when
claiming revision-specific evidence.

Full history admits only observed owner-alone lines, retains all eligible
observed event fields, and fails if they exceed the prompt cap. Its declared
forget recognizer accepts leading "forget"/"erase" requests with an optional
"please" and "my"; matching uses lexical topics and exact keyed fingerprints.
It is not semantic understanding of arbitrary deletion requests. No memory
uses the same control recognizer but retains no dialogue. Probe state includes
fingerprints and is discarded without altering main history.

`lampos_today_partial` calls the real Days protocol and store, passing fixture
time to maintenance and every request. It appends observed exchanges, opens a
fresh session note and searches the store; closed database copies isolate
probes. It does not dispatch voice tools, extract observations, make talk
notes or update the voice running summary. Natural-language forget lines are
ordinary stored input here; only an explicit record-id deletion invokes the
store. Direct record deletion does not report a voice thread clear. This is
**partial current product**, never the current-product baseline. Its missing
voice paths remain visible in inspection output.

The current month plumbing answers all registered follow-up turns and
isolates them from the scripted month. The fixed response contains no tool
calls; live tool-round and rubric-judge orchestration remain separate work.
Speech preparation and a frozen real transcription cache are still required
before any spoken-input result. Recorded month checks use clean text and
cannot satisfy the baseline acceptance check.

For the Tiers adapter's broker and small-cap regression, run the
`tiers_broker_fixture` example with its absolute executable path and a new
absolute state directory. It requires the two synthetic sleep calls to
remain charged across probe restoration and a 500-byte memory-text response
to pass regardless of the runner's separately reported JSON framing. This
fixture contains only a made-up notebook preference and never scores an arm.

## Speech preparation

On a Mac, `speech-prepare EVENTS_JSONL NEW_AUDIO_DIRECTORY VOICE RATE`
synthesizes only to files. For example use an installed `Samantha` voice and
rate `175`. It never selects an output device, plays a file, installs a voice,
or invokes a transcription service. Identical script/voice/rate entries share
one WAV within the preparation. The manifest pins script, WAV and PCM hashes,
duration and synthesis settings; the request plan pins the `say` executable
hash and declares that the voice revision is unreported. Keep these binary
files outside Git and reuse the frozen audio rather than regenerating it.

The parser accepts bounded 16 kHz mono PCM16 WAVE only. The service request
plan follows the product's Nova-3-general websocket settings and sends PCM
frames, not a RIFF header. Live invocation still needs the single budgeted
client's speech transport and the owner's private service configuration.
`Transcript` cache rows distinguish live and recorded provenance, keep
unknown confidence null, and tie the response/request hashes to the exact
audio. Cache application rejects missing, extra, duplicate or changed rows;
there is no clean-text fallback. The cache producer must verify the stored
raw response hash before accepting an imported transcript as live evidence.

The external form is `plumbing-external FIXTURE NEW_RUN ARM RUNNER_REVISION
REGISTRATION_REVISION RECORDED_ANSWER ADAPTER ADAPTER_REVISION`. It pins the
adapter independently of the runner. The Tiers consolidation response is a
fixed empty synthetic proposal: facts, owner-model lines and episodes are
empty. This exercises operation ordering, batching, model brokering and
restoration, not the quality of learned memory. Every model shape records
purpose, input size, output allowance and the fact that usage is synthetic.
Synthetic token usage must never be multiplied by live prices as an invoice
or realistic forecast. Cost estimates use the measured request shapes and
explicit assumptions instead.

Nightly work drains after the final event of each local fixture day; weekly
work drains after every seventh local fixture day. Weekly dates follow
fixture calendar days,
not seven fixed 24-hour intervals. All reference and candidate arms use this
same schedule. The earlier 5a9a1774b reference plumbing attempts predate the
weekly scheduling addition and retain that revision in their manifests.
Recorded main-session forgetting controls precede all ten persistence probes;
probe-local hypothetical operations never establish durable-forgetting credit.

The [human audit list](human-audit-list.json) extracts the fifty pre-selected
questions from the frozen registration, without claiming any completed
review. It also states the additional disagreement/privacy audits. The
[out-of-tree adapter notes](external-arms.md) describe mem0 and MemoryOS
integration requirements without adding either runtime to lampOS.

The [cost estimate](cost-estimate.md) separates measured request sizes from
unmeasured live-path allowances. The [recorded run evidence](recorded-runs/registration-v3/README.md)
retains all fourteen completed month checks and interrupted attempts. These
checks have null scores and do not establish memory quality. Fading is **not
exercised by this month**; the separate aging fixture is registered before scoring.

Registration v4 changes only seat activity and query byte limits. The
[coverage preservation report](development/coverage-revision.json) verifies
unchanged facts, questions, scoring, source links, timestamps and answer keys.
The script provides only `at_desk`, `head_down` and `away`; it provides no topic
labels or object lists. Each second conversation in a visit is scripted head
down; isolated probe visits use a posture rotation independent of query
content. All memory setups receive 4,096 bytes; no-memory must return none.
Full history retains all eligible history as a ceiling and must fit the
pinned model context. Its current 1 MiB transport limit causes refusal, never
truncation; the live context check is still outstanding.

The [minor diagnostics contract](tier-diagnostics.md) keeps prior v2 responses
readable. Counts are metadata only, sampled before each probe and afterward.
Population created by the probe itself gives no population credit. Missing
diagnostics are unknown; an empty or unused full-engine tier makes its
removal not exercised. Proof-only facts are printed separately with zero use
counters. The reports retain the second-known-person and delivered-recall
acknowledgement gaps. The v3 recorded evidence and cost projection stay pinned
to that earlier registration and are not relabelled as v4 results.

The retained [v3 coverage audit](coverage-audit-v3.json) and
[structural counts](month-structure-v3.json) contain no question or answer text.
They distinguish structural opportunity from accepted learned content. The
new seat cues and cap address two gaps; they do not make empty life-story,
owner-model, behavior, ahead or feeling tiers into tested components. Typed
plan expiry and importance-dependent selection also require actual eligible
content. Topic/object labels, a second identified person and delivered-recall
protection remain absent. Sparse aging activity will be scheduled on each
calendar day so nightly/weekly maintenance cannot drift to active-day cadence.

The separate [aging registration plan](aging-plan.md) keeps the month unchanged
and extends the same fictional owner to 120 days. Its 42 fixed probes compare
useful gist, important controls, recent facts and forgetting before/after the
fading boundary; absence of real eligible detail remains unexercised. The
[scenario](aging-scenario.json) is evaluator authoring input, not arm context.

Registration v5 preserves every v4 input/key/question byte and adds the
session-cutting-only candidate after the ten removals. Full Tiers and all ten
removals keep nightly topic segmentation enabled. The no-nightly-sleep setup
means **no consolidation of facts, lines and episode summaries; nightly
topic-boundary pass still runs**. Only the three reference setups may begin
before one agreed segmented build is ready. See the [plan](../../docs/memory-bench.md#topic-segmentation-revision-before-scoring-31-october-2026).

Recorded topic requests use the adapter's pinned boundary-only system prompt.
The provider returns one range covering exactly the offered record order,
including singleton batches and nonconsecutive numeric IDs. It adds no topic
judgment or source text. Changed or malformed topic requests fail closed.
Model-shape rows identify these calls separately from empty consolidation,
so their input size and output allowance can be priced without treating
synthetic token receipts as live usage. These checks never produce scores.
