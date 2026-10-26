# Mood fixture: one paired clock control

On pinned Main `bdc277e2667e11ab04daf60e25f8f9fcf8220dce`, one exact
`the_service_breathes_the_head_with_moods_breath_from_the_bus` case ran old
then new, serially, without retries. **Old failed 0/1/0; new passed 1/0/0**.
Each actual runtime inventory had 28 cases; the other 27 were filtered out.
This is a controlled fixture observation, not a full Expression qualification.

Both private test overlays used `actual_span.max(450_000)` microseconds for
the injected publication-span minimum. **Actual I/O took 1–2 ms**; 450 ms
was synthetic logical time, not an observed host stall. Publisher headers,
Mood's 2.5 s lifespan and transport's 50 ms grace kept their real clocks.
The original amplitude, oscillation, pace and service-cache assertions/bounds
were unchanged. The journal test and product source were unchanged.

Old produced 91 accepted, unexpired Mood observations. Its source state was
250 ms behind the service sample; the live phase was 0.5480769231 when calm
and 0.5600961538 when lively. The original lively assertion failed at
`runtime.rs:494`: pitch **−0.03762630130102758**, against **−0.039 ± 0.001**.
The observed behaviour was breathing, without sleep, waking or drift.

New produced 120 accepted, unexpired observations, all at source age zero and
phase 0.5; its final lively pitch was −0.039. Its staged clock included real
final Mood/probe I/O in later input-receive/tick samples. That nonzero branch
ran in **36/120** samples, advancing by up to 1 ms. Every mapped-wall,
physical-expiry and input-refusal/error check passed.

The probes read actual Latest envelopes before and after the service and
called the public adapter; they did not read or mutate its private cache.
The service itself journaled calm and lively Mood acceptance. These are
separate observations connected by the bound source and due-read cadence,
not a claim of direct private-cache instrumentation.

The old/new commands took 0.734/0.904 s; their different completed work is
not a latency comparison. Both used the same source path and compiler
package identity. Their actual runtime hashes differ; the normal helper
remained byte-identical. All 10,674 source-file, executable and **230 final
sealed-copy** guards passed. Owned process groups closed, the private source
returned to its plain candidate, the permanent worktree was unchanged, and
the controller exited zero and released its lease.

[The audit](audit.json) retains exact source/binary bindings, profiles,
features, counts and receipt hashes. This pair demonstrates the declared
clock counterexample and repair. It does **not** establish the cause of the
earlier full-run failures, waive them, or qualify the later source-only 2bf
carry. The separate unmodified-base pass and all historical failures retain
their original source, binary and workload scopes.
