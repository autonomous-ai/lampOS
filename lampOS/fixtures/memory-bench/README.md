# Memory benchmark fixtures

Everything here is fictional. The development persona has 40 facts, twelve
traits, ten changed facts, two explicit forget requests, six appointments,
visitors and decoys. The generator emits thirty days with fifty utterances
plus session start/end events per day. Its 200 questions include twelve
three-turn appointment probes. No arm has been scored against this material.

Registration version 2 adds three day-9 wrong-person probes, replacing three
visitor disclosure probes while retaining 200 questions. On day 8 a fabricated
second resident supplies three statements while the observed label defaults
to owner. True speaker identity exists only in the evaluator key. The hard
gate is zero owner facts or owner-model lines learned from those records;
missing inspection evidence cannot establish a pass. Version 1 remains in
the repository history; no scored run preceded this change.

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
The appointment probes use a topic cue to disambiguate which plan they test.

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
