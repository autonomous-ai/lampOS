# Kept privacy lease resync fixture: controlled diagnostic

This is a controlled contrast on base `b2344a07d`, staged shipping tree
`22c85356eced4579edae5cab4bcb861092685da8`. It does not identify the cause
of any historical generic “publisher unavailable” failure.

Both private diagnostic variants use the same bounded in-memory instrumentation
and one attempt per branch, with a 30 ms pause after sequence 2 is observed.
They differ only in the fixture's declared publication period. Each calls the
existing destructive notices reader once and records that same vector. No
logging I/O occurs inside the lease window; at most 128 records are buffered.
The paired patches apply to the two-file shipping candidate, not bare Main.

| Variant | Observed pause | Payload age at notice | Header age at notice | Actual notice | Result |
|---|---:|---:|---:|---|---|
| Stock 10 ms | 31.055 ms | 412 us | 0 ms | Late, no SourceGone | 0 passed, 1 failed; exit 101 |
| Fixture-local 1 s | 33.807 ms | 860 us | 1 ms | neither notice | 1 passed, 0 failed; exit 0 |

Both had zero datagrams and reached sequence 4 within the first second;
physical and envelope freshness remained below 50 ms at the notice. The stock
failure is retained as an actual expected negative control, never a passing run.

Commands, run once per private variant from the same dedicated worktree/target:

```sh
cargo +1.96.0 test --offline --locked -p lamp-voice --lib --no-run --message-format=json
# The exact emitted driver, once per variant:
<captured-voice-library> --exact privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second --nocapture --test-threads=1
```

[Independent audit](diagnostic-audit.json) and [terminal receipt](diagnostic-terminal.json)
bind the preserved logs, drivers and source variants. All four owned groups
closed without forced cleanup; per-variant source and raw-input guards held.
Both saved diagnostic drivers and all 65 prior shipping preimages were rehashed.
The controller restored exact shipping source bytes and the unchanged staged
index. The live source and bank have since moved onward; this is no live old-bank
identity claim.

The shipping candidate changes only this synthetic no-datagram fixture's period.
Production's 10 ms period, both 50 ms expiry domains and the one-second resync
bound stay unchanged. The diagnostic does not qualify that shipping source.
Fresh latest-base full Voice, unchanged typed freshness/liveness controls,
formatting, strict Clippy and native gates remain required. Historical causes,
physical behavior and latency have not been established here.

## Shipping source qualification on 4bff

[Independent shipping audit](shipping-4bff-audit.json): full Voice library
3,817/0/15, Msgs library 92/0/0 and full integration 80/0/0, Bus library
55/0/1, and two exact generic Bus liveness controls 2/0/0. The direct
nonoverlapping sum is **4,046 passed, 0 failed, 16 ignored**. The separate
same-three-package docs cohort passes **3/0/0**. All 16 required privacy and
liveness witnesses pass once, and all 11 observed panic hooks are source-expected
(three should-panic, eight caught), with no unknown hook or audit hold.

Formatting, strict all-target Voice/Bus/Msgs Clippy, workspace build and test
compilation passed. Source/raw inputs and all 65 actual/saved executable pairs
remained unchanged in bytes and metadata; all owned groups closed without forced
cleanup or timeouts. This is the stated selected scope, not the full three-package
runtime. The generic Bus Late control uses an 80 ms declaration; it is not a
new stock 10 ms physical-liveness measurement.

[Commit binding](shipping-commit-binding.json) proves the subsequent commit
57f9175ed has exactly tested staged tree fb7e47dc1. Tests ran before that
commit on the identical staged source. An incomplete controller hash was refused
before any test admission; a first auditor admission also failed at sandboxed
index access before log parsing. Both are retained separately; no test was
retried. The unchanged auditor completed under the required index access.

Native ARM64/Linux, two current independent reads and any later base carry are
separate gates. Historical generic-unavailable causes remain unknown.
