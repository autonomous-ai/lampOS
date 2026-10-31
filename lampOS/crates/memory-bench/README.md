# Memory benchmark implementation

This crate is isolated from the product. See the [registered plan](../../docs/memory-bench.md)
and [fixtures](../../fixtures/memory-bench). Recorded plumbing is never an
accuracy score, and the partial current-product arm is never the full baseline.

## Campaign spending boundary

`campaign::Campaign` holds one exclusive OS file lock for a campaign and
persists a hash-chained cost journal. Its immutable plan pins the explicitly
approved cap and a hash of public model/pricing/scope settings. It refuses a
requested plan above the remaining balance. The API is not approval to spend;
the caller must receive the owner's approved settings first.

Attach `Campaign::audit` at the central `model::Client` audit boundary, before
the run-ledger write. A reservation is synced before provider dispatch. If
either write fails, the client refuses dispatch. A known receipt reconciles
once; missing usage retains the full reservation. Only an explicit expiry
before dispatch can release a reservation without usage. A provider charge
above its reservation, or any token accounting limit violation, is recorded
and stops further work even after restart; already incurred
charges cannot be undone.

Creation syncs both files, the campaign directory and its parent before returning.
A new process replays the journal under the lock. Unsettled reservations stay
charged, duplicate call identities cannot spend twice, and a partial/corrupt
journal is refused without automatic repair. The cap and public policy cannot
change on reopen. Every setup, probe, judge, speech call and retry must use the
same campaign account; restoring memory does not restore money.

This accounting module has offline fake-provider tests. Network transport,
environment-only credential loading, duration-priced speech, provenance,
blind live judging and the complete live CLI are subsequent implementation
steps. No live command is enabled by this module. Keys and spending approval
remain required before any paid execution.
