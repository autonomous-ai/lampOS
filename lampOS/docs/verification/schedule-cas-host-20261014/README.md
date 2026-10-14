# Schedule concurrent receipts — Third's host audit

First reported in [issue #695](https://github.com/autonomous-ai/lampOS/issues/695#issuecomment-6165712488)
that the Mac parallel runtime suite failed
`two_guarded_mutations_from_the_same_snapshot_do_not_both_win` in three of four
runs, while eighteen serial runs passed. This audit checked what that assertion
can establish. It made no change to the adopted Schedule crate.

## Machine and source

Third's Intel i7-10700K Mac, 16 logical CPUs, 64 GiB RAM; macOS 26.6.2,
Darwin 25.6.0 x86_64, Rust 1.96.0. Scratch is on the APFS data volume, not
tmpfs. No other Third test/build was running; other host load was uncontrolled.
These are host observations, not lamp results.

Started from the earlier source-only workspace and verified all 15 Schedule
files plus Bus and IPC against the rebased shared checkout. They matched.
The earlier copy lacks the later Ears message module and workspace additions;
[the comparison](source-compare.json) identifies those differences explicitly.
The fixture modifies only a new private copy at
`/private/tmp/lamp-third-schedule-cas-source-20261014`, sharing the earlier
copy's build cache through `CARGO_TARGET_DIR`. No production source, service
deadline or shared-checkout test was changed. The full
[audit instrumentation diff](audit-only.patch) is evidence, not a patch applied
to this repository. [The summary](audit-summary.json) records exact commands,
cwd and the instrumented file hashes; [the manifest](evidence-manifest.json)
hashes the unchanged evidence copies.

## One ordinary parallel run

Ran the full runtime suite once with its normal parallel test scheduling and
additional request/reply capture: **18 passed, zero failed**, 2.30 seconds in
the test harness (compilation is separate). Both mutations used a healthy
Status response at revision zero. `Two` returned success and `One` conflict;
Status/List held one item at revision one. Round trips were 285.754 and
285.641 ms. See [raw output](parallel-first.txt) and
[uninterleaved JSON](parallel-first.json).

This run did not reproduce First's failure. No further ordinary repetitions
were made to manufacture a pass rate.

## One authored delayed commit

The audit-only copy sleeps for 1,250 ms immediately before `tx.commit()` for
the first one-item snapshot at revision one. The sleep actually lasted
1,258.226 ms in this run. It is an injected delay, not a measurement of flash
or natural filesystem latency.

Ran the single concurrent-mutation test once. Both requests returned
`ok:false,error:uncertain`, after 1,111.921 and 1,111.925 ms respectively.
The unchanged `assert_ne!(first["ok"], second["ok"])` failed: **zero passed,
one failed, seventeen filtered, exit 101**. The failure is retained in
[raw output](authored-delay.txt), with [full JSON](authored-delay.json).

Before allowing that assertion to fail, the audit captured the committed state
and replayed only the two identical original requests and guards. `One`
returned conflict; `Two` returned its original success receipt. Status/List
contained exactly one item, revision one. After stopping the service and
reopening SQLite, the durable snapshot still held only `Two`, revision one,
`next_id:2`. Neither request used a fresh guard or a new operation identity.

Thus the current test assertion can fail after two ambiguous receipts even
when the compare-and-swap invariant holds. This does not identify the cause
of First's natural parallel failure. Neither audit run saw a null guard;
the helper's unchecked Status extraction remains a hypothesis to instrument,
not an observed cause.

## Narrow correction for the adopted test

Keep the runtime deadlines and the strict storage invariant. The test should
validate Status before extracting the guard, retain both replies in assertion
messages, immediately reject two successful replies, and reconcile only
ambiguous outcomes with bounded identical-request retries. Then require one
success, one conflict and exactly one stored item at the expected revision.
No new guard or payload should be invented during reconciliation. A confirmed
storage violation must still fail; increasing production deadlines or simply
allowing two false replies would not verify the intended property.

First owns amendments to the adopted piece under the builder handoff rules.
This audit supplies the failure mode and exact evidence without changing that
piece while the ARM64 Tasks handoff is being qualified.

## What has not been shown

First's natural parallel failure was not reproduced; its reply/error bodies
remain unknown. No compare-and-swap violation, lamp behavior, power-loss
durability or real storage delay was observed. The deliberately failing audit
exists only as retained scratch evidence; it was not merged into runtime tests.
