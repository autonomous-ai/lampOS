# Live-plan cost estimate, 31 October 2026

These are planning estimates from **recorded plumbing**, not billed usage,
accuracy results or a promise that every live run fits the estimate. All
fourteen setups completed registration v3 with the same fixture. No service
was called. The partial current-product row is not the complete product baseline.

Prices use peak, cache-miss [DeepSeek list rates](https://api-docs.deepseek.com/quick_start/pricing/):
Flash $0.30 per million input tokens and $1.20 output; Pro $1.32 input and
$3.96 output. Model choice still requires approval. Input estimates apply the
provider's [approximate 0.3 token per English character](https://api-docs.deepseek.com/quick_start/token_usage/)
to serialized recorded requests. JSON framing and tokenization differ from
that heuristic. Actual returned usage is authoritative; synthetic receipts
with placeholder usage are never priced as live tokens. No caching discount,
off-peak discount or free credit is assumed.

## Recorded v3 request shapes, one repetition per setup

Input and output columns are model-only millions of tokens. Outputs use the
registered maximum output allowance per recorded call, not the tiny synthetic
reply. Dollar columns also include the same blinded-judge planning allowance.

| Setup | Input estimate (M) | Output allowance (M) | Flash + judge | Pro + judge |
|---|---:|---:|---:|---:|
| No memory | 0.0327 | 0.1147 | $1.04 | $4.39 |
| Full history | 24.6285 | 0.1147 | $8.42 | $36.86 |
| Partial current product | 0.0336 | 0.1147 | $1.04 | $4.39 |
| Ours (tiers) | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Without episodes | 0.4151 | 0.2867 | $1.37 | $5.58 |
| Without life story | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Facts not recalled; evidence kept | 0.4142 | 0.2867 | $1.37 | $5.58 |
| Without owner model | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Without behavior memory | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Without upcoming plans | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Without feelings | 0.4321 | 0.2867 | $1.37 | $5.60 |
| No consolidation of facts, lines and episode summaries; nightly topic-boundary pass still runs* | 0.2944 | 0.1638 | $1.18 | $4.93 |
| Without importance | 0.4321 | 0.2867 | $1.37 | $5.60 |
| Without fading | 0.4321 | 0.2867 | $1.37 | $5.60 |

*The row uses the older no-sleep recorded request shape, when the topic pass
did not exist. Its new topic-boundary cost is unmeasured and is not included
in that historical dollar value. All candidate rows retain their v3 pins.

The judge allowance covers 76 rubric questions, 175
criteria and 100 answer/follow-up turns per run:
2,835,030 estimated/allowed input tokens and
38,912 output tokens. It includes all eligible observed
source text, the fixed rubric, up to 512 tokens per candidate answer, and 512
instruction plus 512 output tokens per judgment. This is a conservative prompt
sizing assumption; a live judge has not been run or its final prompt frozen.
Human review and compute/storage costs are not priced here.

## Unmeasured work and planning allowances

The complete product's observation, talk-note and summary paths are absent
from the partial store arm. Allow 254 main observation
calls and 1500 combined main summary/final-fold
calls. Isolated probes may repeat inherited work; allow a further
448 observations and 1120
folds. These counts are assumptions pending the product clock/drain hook, not
proven execution bounds. Each observation is budgeted at 16k input/4k output
and each summary at 6k input/4k output. They add $24.03
Flash or $88.20 Pro per complete-product run. The current
product does not send an output-token ceiling on these calls.

For complete-product answers/tools, replace the partial arm's measured model
cost with an allowance of eight requests for each of 224 answer turns, at
16k input and 512 output tokens per request: $9.70
Flash or $41.48 Pro per run. The 16k input assumption
is not a new cap or permission to truncate memory. None of the other current
in-tree adapters advertises model-selected tools.

Recorded Tiers consolidation returns empty proposals. Learned memory may
increase subsequent prompts and batch counts. Add twice each candidate's
measured model-only cost as a separate sensitivity allowance, giving three
times the structural model estimate plus the unchanged judge allowance.
That factor is not a proven live-cost bound. Add 25% contingency for request
variation and retries. Every retry remains an explicit retained attempt;
there is no authorization here to spend money or retry silently.

## Current plan: fifteen setups, 45 runs per persona

Before any score, registration v5 adds **session cutting only**, after the
existing ten removals. The full engine and all ten removals keep nightly
topic segmentation enabled. The no-nightly-sleep setup means **no consolidation
of facts, lines and episode summaries; nightly topic-boundary pass still runs**.
Weekly reflection remains enabled in that setup.
Its added boundary-pass cost, and segmentation overhead in the other candidate
setups, remain unmeasured. The projection below excludes that new overhead;
the campaign cap must stop the run if the allowance is exhausted.

The boundary-only allowance is now registered as 64 plus the compact JSON
byte length of one singleton range per offered record: equivalently 78 plus
39 and twice the decimal ID length for each record. Six one-digit IDs allow
324 tokens. The adapter shortens batches at its configured ceiling and checks
both returned bytes and reported tokens. Actual offered batches from the new
pinned adapter are needed to replace the unmeasured overhead; the old pass
contains no such calls. Do not substitute the synthetic response length.

The extra setup uses the prior unsegmented full-engine shape as its estimate:
432,119 input tokens and 286,720 output tokens allowed
per run, plus the same judge reserve. That is $1.37 Flash
or $5.60 Pro per run. Six extra runs across the two personas add
$8.23 Flash before allowances, or $17.39 with the same learned-state
allowance and 25% contingency. This is a projection, not a measured new run.

| Requested scope | Runs | Flash, with contingency | Pro, with contingency |
|---|---:|---:|---:|
| Development month | 45 | $268.04 | $1053.77 |
| Held-back repetition, assuming equal workload | 45 | $268.04 | $1053.77 |
| Both personas | 90 | $536.07 | $2107.54 |
| Optional mem0 and MemoryOS, both personas | 12 extra | Not estimated; excluded | Not estimated; excluded |
| Separate registered 120-day aging test | 6 extra | Not estimated; excluded | Not estimated; excluded |

**Proposed owner decision remains a $600 Flash campaign cap, split into $300
per persona.** The $536.07 projection does not authorize spending or guarantee
completion. The separately registered aging test and optional outside systems
need separate estimates and budget decisions. If Pro is selected, the
corresponding two-persona estimate is $2107.54; the previous rounded $2,400
proposal remains above it, also without a completion guarantee.

For comparison, the prior fourteen-setup projection was $259.34 Flash per
persona ($518.69 for both), or $1019.97 Pro per persona ($2039.93 for both).
Its measured rows, original totals, revisions, hashes and retained ledgers
remain unchanged in the machine-readable estimate. No earlier run is
relabelled as evidence for the new engine or the new registration.

Fading is **not exercised by this month**: the existing mechanism starts only
beyond 90 days, while the month spans about 29 days. Its removal still runs
and is reported, but this month cannot decide whether fading stays. The separate
[120-day aging fixture](aging-plan.md) is registered before live scoring.
No threshold or registered month content is changed to fit the test.

Speech is charged once per persona and reused across every setup/repetition.
The unchanged 1,500 owner recordings total 51.854
minutes. [Deepgram Nova-3 monolingual streaming](https://deepgram.com/pricing)
costs $0.0077/minute plus $0.0013/minute for the configured keyterm prompting:
$0.467 for the full record list. Deduplicating the
191 identical audio files reduces this to 9.617
minutes and $0.087; totals deliberately use the
larger amount. Local file-only synthesis has no cloud-TTS charge. Audio hashes
match the revised owner script, but no real transcription has been performed.

The held-back persona is not authored, so its projection assumes the same
size and workload. Optional mem0 and MemoryOS add six runs per persona;
their extraction/embedding/storage costs remain unmeasured and are excluded.

The owner must set a spending ceiling separately. The runner must stop before
a reservation exceeds the approved run/campaign balance and retain incomplete
results; an estimate does not guarantee completion. Real model identity,
prices, budgets and transcript cache must be pinned before live evaluation.

The [machine-readable estimate](live-cost-estimate.json) records per-call
shape totals, revisions, fixture hashes, assumptions and every retained
attempt. [Run evidence](recorded-runs/registration-v3/index.json) includes the
completed and disk-interrupted ledgers. These are plumbing checks with null
scores throughout, not any of the requested live baseline numbers.
