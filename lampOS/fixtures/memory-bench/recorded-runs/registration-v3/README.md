# Recorded month evidence: registration v3

These are **plumbing checks only**, not memory scores. Fourteen setups each
completed one month: 2,220 events, 200 questions, 224 answer turns and thirty
nightly operations. The three reference setups use runner/adapter `3d8be60e8`;
all eleven Tiers setups use adapter `66b96bb73` with that same runner. The
manifests pin full revisions, executable hashes and the unchanged registration.
Every score is null. Model replies and consolidation proposals are synthetic;
no model or transcription service was called. No-fading is **not exercised by
this month**, whose duration is below the unchanged 90-day threshold.

The [index](index.json) includes sixteen attempts: fourteen complete and two
interrupted when the shared disk filled. The partial-current-product and
facts-removal retries use new attempt directories. Interrupted bytes remain
unchanged. Their terminal failure records and hash chains verify; the index
labels them failed. They are neither passing checks nor missing experiments.
No earlier nine-minute-timing attempt has been substituted into this pass.

Each exported manifest and ledger is byte-for-byte identical to its retained
source. The index records file sizes and SHA-256 hashes. All sixteen original
ledgers passed `verify-ledger` before export; the terminal status distinguishes
the fourteen completed checks from two failed attempts. Binary executables, audio and databases are not committed.

The verifier requires private files, whereas Git checks files out with ordinary
read permissions. To verify a completed exported attempt, copy only its
manifest and ledger into a new private directory, then run the pinned runner:

```sh
attempt=fixtures/memory-bench/recorded-runs/registration-v3/burst-none-3d8be60e8
verification_dir=$(mktemp -d)
chmod 700 "$verification_dir"
install -m 600 "$attempt/manifest.json" "$verification_dir/manifest.json"
install -m 600 "$attempt/events.jsonl" "$verification_dir/events.jsonl"
lamp-memory-bench verify-ledger "$verification_dir"
```

Use the executable built from the manifest's runner revision when claiming
revision-specific verification. Keep that private copy until verification is
complete. `plumbing-card` can combine completed private copies; it prints
counts by question kind with null scores.

The [cost estimate](../../cost-estimate.md) uses recorded request sizes and
explicit unmeasured-path allowances. Placeholder recorded usage is never
billed or interpreted as actual token consumption. The held-back persona,
aging test, live speech cache, full product hook and scored runs remain open.
