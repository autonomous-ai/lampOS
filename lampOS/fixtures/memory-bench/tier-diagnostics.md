# Arm contract 2.1: optional inspection diagnostics

Minor revision only: the wire envelope stays `v: 2`; all prior response shapes
remain readable. Add one optional `diagnostics` member to `inspect` response
`data`. It never reaches an answer, extraction or judge prompt and is never
scored. Missing diagnostics or a missing tier means unknown, never zero.

```json
{
  "diagnostics": {
    "schema": "tier-usage-v1",
    "tiers": {
      "episodes": {
        "items": 240,
        "text_bytes": 10240,
        "recall_calls": 7,
        "snapshot_calls": 0
      },
      "life_story": {
        "items": 0,
        "text_bytes": 0,
        "recall_calls": 0,
        "snapshot_calls": 0
      }
    }
  }
}
```

Each metric is an unsigned integer or null when the adapter cannot measure
it. Tier identifiers for Tiers are `episodes`, `life_story`, `facts`,
`owner_model`, `how_to_be`, `ahead`, `feelings`; adapters may add their own
identifiers such as `record`, `full_history`, `talk_notes`, `running_summary`.
The metric object has exactly these four members; no text, source identifiers,
queries, claims, tags or deleted-word fingerprints are permitted.

`items` and `text_bytes` describe the current isolated store's eligible tier
content at inspection time, excluding evidence-only fact rows in the facts
removal. They are occupancy, not cumulative creation totals. UTF-8 bytes count
stored tier text, not SQLite/index/JSON overhead; null is better than a guess.

`recall_calls` and `snapshot_calls` are cumulative since init, counting calls
that returned at least one nonempty text item from that tier after the adapter's
byte-cap selection. Count a tier once per call, not once per item. These
counters survive probe restoration and are diagnostic-only accounting: never
use them for ranking, importance, aging or memory decisions. They do not mean
that the runner accepted a late response or that speech was delivered.

Bench inspects the main store before each probe and the isolated store after
it; diagnostics are retained in the ledger. Population eligibility uses the
pre-probe sample, avoiding credit for facts created by the question itself.
Usage deltas come from the two samples and include both recall and snapshot.
Report a removal as `not exercised` when its full-engine counterpart never
has eligible population or never supplies text to memory prompts. Missing
measurements mean `unknown`. This classification cannot turn an equal score
into proof that a tier has no value. The runner separately reports deadline
misses and delivered-recall acknowledgement remains an explicit gap.

The `evidence_only_facts` identifier is permitted only in the facts-removal
setup. It reports retained proof rows separately from the disabled `facts`
tier. Both use counters are always zero; this is not recallable fact content.
