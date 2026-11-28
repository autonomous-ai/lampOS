# Vision EVAL, public/synthetic v1

Use for #8847, #8848 and #8964; link the result on #8850. Keep the morning
rubric/table unchanged. The [protocol](../../docs/vision-evaluation-20261128.md)
and [recipe](vision-freeze-20261128-v1.json) define the planned counts.

```text
EVAL <sensor/model/backend> — <time GMT+7>
Rubric: vision-public-synthetic-20261128-v1
Recipe SHA256: ...
Input seal: complete / partial / not materialized; planned and present counts
SET_BAR: PASS / FAIL / NOT_RUN, independently for EACH source/class/cell
CONVERSION: PASS / FAIL / NOT_RUN, exact vs approximate parity stated
STREAM: PASS / FAIL / NOT_RUN, short vs sustained stated

Sources: origin, publisher/asset licence URL and snapshot hash, attribution,
         commercial training/calibration permission, known prior exposure
Roles: train/tune/test component-map hash; identities/voices/utterance families;
       calibration IDs/tensor hashes; no cross-role sources
Geometry/labels: measured or proxy camera; exact projected/annotation basis;
                 missing/unknown fields, face counts, shoulders vs head
Quality: correct/planned for every required class and cell; exact integer bar;
         wrong, unknown, missing, unavailable and late; no removed primary rows
         per-identity/session and reused-source slices; independent episodes
Conversion: CPU and candidate input hashes; original FP32/converted graphs;
            preprocessing, shape/dtype/layout/range; compiler/options;
            quantization/dequantization; raw errors and decision flips;
            correct-both / gained / lost / wrong-or-unknown-both
Timing: exact input→output boundary; cold/context-fill; warmup and measured n;
        fresh-decision p50/p95/max; throughput, queue/drop/repeat/deadlines;
        detector + mesh + all faces + postprocess costs; no window amortization;
        actual NPU/CPU operators, fallback; thermal/load/frequency and voice impact;
        null replay readiness, physical source-clock uncertainty if measured
Provenance: source/base/full patch, actual binary, models/configs, renderer,
            frozen input/label/case/calibration manifests, run/report/scorer hashes
Retention: every raw/failed/source/derived artifact, README and inventory SHA256;
           dataset total bytes <5,000,000,000; originals remain local
Limits: generated/public/annotation-proxy status; no real-person or hardware
        claim beyond the evidence; no score assigned to an unrun model/cell
Default-off; no merge/release train. Lead alone runs board legs.
```

A result may pass the named set while actual lamp performance is unmeasured.
Monday's footage is a later external test, not permission to begin today's
public/synthetic work. No change to thresholds, labels, crop/sign convention
or test membership after predictions without a new version and exposure note.

