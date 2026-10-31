# Live-plan cost estimate, 31 October 2026

These are planning estimates from **recorded plumbing**, not billed usage,
accuracy results or a promise that every live run fits the estimate. All
fifteen setups completed registration v5 on runner `6420a2f49`, with every
Tiers setup using adapter `7d6d0a236`. No service
was called. The partial current-product row is not the complete product baseline.

Prices use peak, cache-miss [DeepSeek list rates](https://api-docs.deepseek.com/quick_start/pricing/):
Flash $0.30 per million input tokens and $1.20 output; Pro $1.32 input and
$3.96 output. Model choice still requires approval. Input estimates apply the
provider's [approximate 0.3 token per English character](https://api-docs.deepseek.com/quick_start/token_usage/)
to serialized recorded requests. JSON framing and tokenization differ from
that heuristic. Actual returned usage is authoritative; synthetic receipts
with placeholder usage are never priced as live tokens. No caching discount,
off-peak discount or free credit is assumed.

## Recorded v5 request shapes, one repetition per setup

Input and output columns are model-only millions of tokens. Outputs use the
registered maximum output allowance per recorded call, not the tiny synthetic
reply. Dollar columns also include the same blinded-judge planning allowance.

| Setup | Input estimate (M) | Output allowance (M) | Flash + judge | Pro + judge |
|---|---:|---:|---:|---:|
| No memory | 0.0327 | 0.1147 | $1.04 | $4.39 |
| Full history | 24.8482 | 0.1147 | $8.49 | $37.15 |
| Partial current product | 0.1001 | 0.1147 | $1.06 | $4.48 |
| Ours (tiers) | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Without episodes | 0.5727 | 0.3733 | $1.52 | $6.13 |
| Without life story | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Facts not recalled; kept only as evidence | 0.5717 | 0.3733 | $1.52 | $6.13 |
| Without owner model | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Without how-to-be | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Without what is ahead | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Without feelings | 0.5896 | 0.3733 | $1.52 | $6.15 |
| no consolidation of facts, lines and episode summaries; nightly topic-boundary pass still runs | 0.4526 | 0.2504 | $1.33 | $5.49 |
| Without importance | 0.5896 | 0.3733 | $1.52 | $6.15 |
| Without fading | 0.5896 | 0.3733 | $1.52 | $6.15 |
| session cutting only | 0.4321 | 0.2867 | $1.37 | $5.60 |

The judge allowance covers 76 rubric questions, 175
criteria and 100 answer/follow-up turns per run:
2,835,111 estimated/allowed input tokens and
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

Registration v5 adds **session cutting only**, after the existing ten removals.
The full engine and all ten removals keep nightly topic segmentation enabled.
The no-nightly-sleep setup means **no consolidation of facts, lines and episode
summaries; nightly topic-boundary pass still runs**. Weekly reflection remains
enabled in that setup. All fifteen setups now have measured recorded request
shapes from one pass; the synthetic answers remain unscored.

The full-engine month has 240 boundary-only calls reserving 86,583 output
tokens, ranging from 160 to 642 per call. Its request hashes and shapes match
an independent run on the same binaries exactly. The allowance is 64 plus
the compact JSON byte length of one singleton range per offered record:
78 plus 39 and twice the decimal ID length for each record. The adapter
shortens batches at its configured ceiling; it checks compact partition size
and reported usage, allowing equivalent pretty-printed JSON within the token
allowance. The budget uses these maximum allowances, never the shorter fixed
recorded reply. Boundary calls are now included in every segmented setup.

| Requested scope | Runs | Flash, with contingency | Pro, with contingency |
|---|---:|---:|---:|
| Development month | 45 | $286.99 | $1123.03 |
| Held-back repetition, assuming equal workload | 45 | $286.99 | $1123.03 |
| Both personas | 90 | $573.99 | $2246.06 |
| Separate registered 120-day aging test | 6 extra | $29.66, excluded above | $106.13, excluded above |
| Month plus aging | 96 | $603.64 | $2352.19 |
| Optional mem0 and MemoryOS, both personas | 12 extra | Not estimated; excluded | Not estimated; excluded |

The **proposed $600 Flash month cap**, split into $300 per persona, remains
unapproved. The month projection is $573.99; adding the separate aging
scope brings it to $603.64. Neither estimate authorizes spending or
guarantees completion. Aging and optional outside systems require separate
budget decisions. The Lead has asked the owner whether to include aging under
a $610 Flash cap; that extension is **unapproved**. Until the owner answers,
the active planning scope remains the two months only. If Pro is selected,
the two-persona month estimate is
$2246.06, or $2352.19 including aging.

The [aging estimate](aging-cost-estimate.json) uses an independently supplied,
verified full-engine ledger and assumes equal workload for no fading. It
includes its own 32-question judge reserve, speech cache, learned-state
allowance and contingency. Its recorded consolidation is empty and provides
no fading result. Its longest measured audio exceeds the pinned runner's
60-second bound; bounded longer-audio support is required before scoring.

The earlier 14-setup v3 totals ($518.69 Flash / $2039.93 Pro), the provisional
15-setup projection excluding topic overhead ($536.07 / $2107.54), their
original rows, revisions, hashes and retained ledgers remain unchanged in the
machine-readable estimate. No earlier run is relabelled as evidence for v5.

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

The [machine-readable estimate](live-cost-estimate.json) records current and
historical request shapes, revisions, fixture hashes and assumptions.
[Current run evidence](recorded-runs/registration-v5/index.json) retains all
fifteen completed ledgers; [earlier evidence](recorded-runs/registration-v3/index.json)
also retains the disk-interrupted attempts. These are plumbing checks with null
scores throughout, not any of the requested live baseline numbers.
