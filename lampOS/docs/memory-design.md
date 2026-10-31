# Lamp memory: the tiered design

31 October 2026. Approved design direction, based on the supplied memory
architecture draft 2 and its owner-first draft. This is a design to evaluate,
not a description of features already shipped. Its name is independent of
lampOS release numbers.

The first phase is an isolated Rust benchmark and candidate library. It does
not change `crates/memory`, `crates/voice`, deployment, or a lamp. The
[benchmark plan](memory-bench.md) defines the evidence needed before product
integration is proposed. The current implementation is documented in
[memory.md](memory.md), [memory-days.md](memory-days.md), and
[second-voice.md](second-voice.md).

## What exists and what we take

The supplied research suggests separating fast recording from slower
consolidation. These are design precedents, not evidence that this design
wins on a lamp. Published scores use different data, models, and judges;
they are not the benchmark's baseline numbers.

| Source | Idea to evaluate here |
|---|---|
| [MemoryOS](https://arxiv.org/abs/2506.06326) | Recent dialogue, intermediate organization, and lasting personal memory, with explicit transitions |
| [LightMem](https://arxiv.org/abs/2510.18866) | Group by topic; consolidate outside the live answer path |
| [MIRIX](https://arxiv.org/abs/2507.07957) | Distinguish episodic, semantic, and procedural memory instead of treating all text alike |
| [MemGPT](https://arxiv.org/abs/2310.08560) | A small persistent context with larger stores recalled as needed |
| [Generative Agents](https://arxiv.org/abs/2304.03442) | Test whether reflection and importance improve behavior through ablations |

Keep lampOS's local storage, person boundaries, real forgetting, explicit
process interfaces, fixture clock, and existing regression evidence.
Evaluate new retention and consolidation in a separate store. Reuse suitable
Rust and SQLite facilities; do not import a Python runtime or make live chat
depend on the experiment. Every tier must improve measured results or be
removed. The raw record remains the source, not a model-generated summary.

## The owner-first rules

The lamp is a companion beside one owner at a desk. It should use what it
remembers quietly, as a considerate companion would, rather than reciting a
profile or announcing that it remembers.

- Keep what the owner says **to the lamp**, including an unanswered line,
  until the owner forgets it. Do not treat overheard calls or room talk as
  consent to store them.
- The proposed attribution policy defaults to the owner only when no signal
  indicates another person. Another voice, another face, or an unknown face
  is contrary evidence. Uncertain attribution remains uncertain; it cannot
  establish an owner fact or owner-model line.
  A default owner label is not verified identity: a second resident alone
  at the desk can receive that label when the camera is unsure or off. The
  benchmark registers that failure case with evaluator-only true identity
  and requires zero owner facts or owner-model lines derived from those
  records. This risk is not accepted or waived by the design; it must be
  measured and resolved before a product integration can satisfy the gate.
- An enrolled known person retains a separate small memory. An unknown
  visitor's statements do not become owner memory. Visitors receive none of
  the owner's private memory. Sharing a desk never grants access.
- Health, money, and relationship information can belong to the owner's
  memory, but must not be disclosed with others present. Unknown presence
  is not proof that the owner is alone.
- The owner can inspect, correct, forget, pause, and export memory. A pause
  prevents new retention, including queued consolidation and derived copies.
  An export contains only the owner's own memory, never an enrolled person's
  separate memory or private information retained about other people.
- Persistent memory is text on the lamp. No raw audio, pictures, faces, or
  screens become memory artifacts. Text sent for inference goes through the
  existing relay; provider and relay retention must be established before
  real-owner trials.

These are requirements for the candidate, not permission to weaken the
current product's attribution or disclosure rules. Enrollment, voice
identification, and the desk-observation switch remain separate product
work. The benchmark supplies observed attribution and presence, including
mistakes, without giving an arm the hidden true speaker.

## The stores

| Store | Contents | Intended lifetime |
|---|---|---|
| Sensory | Transient sound, camera frames, partial words | Seconds; outside persistent memory |
| Working | Recent conversation, running summary, current speaker and presence, activity, desk cues, topic, goal, and recalled context | The conversation |
| Short term | Today's episodes, with full text, times, participants, topic, feeling, open loops, and importance | Until consolidation; provenance remains in the record |
| Long term | Life story, facts and people, owner model, how-to-be rules, future plans, and feeling tags | Months to years, subject to correction and forgetting |
| Record | Verbatim directed owner lines and the lamp's replies, with timestamps and attribution | Until forgotten; source for derived memory |

The record is not another lossy tier. Summaries can be rebuilt from it, but
never from a portion that was forgotten. A quote means the retained
speech-to-text transcript, not a claim that transcription was acoustically
correct. Keep speech-to-text confidence and attribution beside it.

### Long-term parts

- **Owner model:** a compact, approximately 1–2 KB snapshot of identity,
  relationships, tastes, work style, values, goals, worries, and preferred
  behavior. Every line cites facts. Code removes a line when its supporting
  facts are gone. Size and selection are tested, not evidence of correctness.
- **Life story:** chapters for periods or themes, events within them, and
  moments backed by record quotes. An episode has time, people, place, topic,
  gist, feeling, importance, and source ids. Old, low-importance detail may
  fade while its gist remains. Fading is distinct from requested deletion.
- **Facts and people:** claims with source record ids, code-copied quotes,
  speaker, time, transcription confidence, strength (`said`, `maybe`, or
  `plan`), kind, validity interval, expiry rule, and supersession link.
  Only the owner's own eligible words establish a fact about the owner.
  New information retires the old claim as history; uncertainty stays visible.
- **How to be:** taught rules stored word for word, plus learned patterns
  grounded in corrections and reactions. A learned preference must remain
  distinguishable from an explicit instruction.
- **What is ahead:** plans, promises, events, reminders, and one appropriate
  follow-up. An appointment closes after its day; an unused plan expires
  after 14 days. Likes and habits do not expire solely because of age.
- **Feelings:** the owner's stated feelings and clearly attributed tone
  observations, linked to episodes. An inference about mood is not a
  diagnosis or a quoted fact.

People and places form links between records, facts, and episodes. No
unattributed model invention can become a relationship in that graph.

## How memory changes

1. **Take in.** Eligible directed speech reaches working memory and the
   record. Fold older exchanges into a running summary as the working
   budget fills. Keep uncertain attribution separate from eligible evidence.
2. **Cut episodes.** End an episode on topic change or the owner leaving,
   rather than a fixed 45-second quiet interval. Mark importance from an
   explicit request to remember, strong feeling, first mentions, plans,
   promises, names, dates, and repetition. Small talk is usually low priority.
3. **Remember explicitly.** A direct request uses a fast path and is read
   back to check mishearing. Do not confirm a durable save before it has
   succeeded. Slow consolidation is independent of immediate feedback.
4. **Sleep nightly.** Prepare bounded input from the day's episodes and
   eligible existing memory. DeepSeek through the relay returns fixed JSON:
   proposed facts, episode organization, owner-model changes, behavior
   patterns, and plans. An empty result is valid. Code validates the result
   and applies it atomically; the model only points at record ids, and code
   copies all quotes. Check forget fingerprints before any proposal applies.
5. **Reflect weekly.** A bounded pass can propose broader patterns and open
   or close chapters. The same evidence, attribution, budget, and forgetting
   checks apply. A proposed nightly rewrite dropping more than a quarter of
   the existing owner model is refused rather than silently replacing it.
6. **Recall from cues.** Use observed people, time, activity, words heard so
   far, and desk objects to find linked chapters, people, and episodes.
   Rank with keyword search, recency, and importance. Questions about exact
   past words can search the record as well. Measure each ranking component.
7. **Forget.** A request deletes the selected source and dependent content,
   including cached or queued derivatives. Retain only a keyed HMAC
   fingerprint needed to prevent re-learning, with its key in a separate
   private file. The database alone cannot confirm a guessed value; the
   database and key together still permit that test. Reset deletes the key
   as well as the store. Later sleep, retries, replay,
   and reconstruction must not restore the forgotten content.

The model never deletes records. Deterministic, reviewed code owns expiry,
requested deletion, and any permitted fading. Fingerprint representation,
scope, correction behavior, and retention need adversarial tests: retaining
the forgotten words as a fingerprint would not constitute forgetting.

An unavailable model leaves existing memory valid; retry consolidation on a
later bounded schedule. Explicit remembers must still work. Input sizes,
worker count, pending work, retry count, timeouts, and model cost all have
limits. Cancellation must discard late results for a closed or forgotten
scope. A failed or refused update is recorded, not counted as a successful
night.

## Intended product boundary, after evaluation

The eventual store belongs in the memory process, backed by SQLite. Working
conversation state belongs in voice. The design uses a session-start owner
snapshot and cue-driven recall; it does not require new live model tools or
exceed the second route's current **48-tool limit** (`second_provider.rs`);
the 32-tool limit applies to Gemini Live. Integration must preserve compatibility with
the previous build on each side of the process interface.

Prepare the snapshot outside the first-answer critical path. Start recall
while speech is in progress and use it only if ready at the existing answer
deadline. The target is **no measurable added first-audio delay** versus the
same route without recall. Late or failed retrieval must not block capture,
interruption, or the first spoken answer. Measure cold first use, repeated
turns, and slow or failed dependencies. Host recall timings alone cannot
establish that target on a lamp.

Natural behavior is part of acceptance: apply preferences quietly; ask a
follow-up once when appropriate; accept stillness and silence; never read
out the owner model, invent a fact, or turn recall into an unasked interview.

## Build order and release ruling

1. Commit the fake month, questions, and scorers before any arm result.
2. Build the separate benchmark and measure the existing baselines.
3. Build the isolated tiered store and its ablations against the same contract.
4. Compare on the held-back persona, audit the scores, and review privacy,
   forgetting, costs, and failure behavior.
5. Only then propose product integration, measure acoustic latency on an
   authorized lamp, and test the Monday-said/Tuesday-used behavior.
6. App inspection, correction, pause, export, desk observation, and voice
   identification need their own assigned work and acceptance evidence.

The owner approved building the new design on 31 October. The first builder
[accepted Bench-P1 on the Queue](https://github.com/autonomous-ai/lampOS/issues/4710#issuecomment-6541830902):
new crates and docs only, product memory, voice and deployment unchanged,
no product links, and fictional people only. The current `AGENTS.md` release
exclusion therefore remains in force: this is an experiment until its scores,
reviews, and the owner's integration decision. The proposed clarification,
limited to this phase, was:

> Keep Harness, agent sessions, and browsing out of this release. Allow the
> isolated memory benchmark and tiered-memory library experiment approved on
> 31 October 2026. Do not wire the new memory into product voice, memory, or
> deployment until its benchmark and integration gates are accepted; live
> chat must not depend on the experiment.

The ruling permits the isolated experiment without changing `AGENTS.md`.

## What was not shown

No candidate score, production integration, device behavior, voice-identity
reliability, acoustic latency, provider-retention guarantee, or two-week
real-owner result is established here. The desk-observation choice remains
provisional: the source draft recommends an explicit off switch. The held-back
persona must be written and controlled by someone outside the implementers.
Permanent retention, derived-copy deletion, and the exact consent and
attribution boundary need review before any real-owner data is used.
