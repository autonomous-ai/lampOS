# Memory benchmark fixtures

Everything here is fictional. The development persona has 40 facts, twelve
traits, ten changed facts, two explicit forget requests, six appointments,
visitors and decoys. The generator emits thirty days with fifty utterances
plus session start/end events per day. Its 200 questions include twelve
three-turn appointment probes. No arm has been scored against this material.

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

`eligible_evidence_ids` allows citations to any observed past record;
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
