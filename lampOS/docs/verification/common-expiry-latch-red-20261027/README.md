# Common original-speaker expiry negative control

Ready for lamp-voice: no

[Receipt](receipt.json) for local source `67c500d29`, containing the corrected
control 112 and timing instrumentation with the old production latch unchanged.
This is source-scoped negative-control and retention evidence for
[#3064](https://github.com/autonomous-ai/lampOS/pull/3064), not a corrected-code
or release qualification.

The intended regression actually exits 101: **0 passes / 1 failure**, with
3,996 filtered cases not executed. It fails at `worker.rs:22293`, the first
Pending-at-expiry loss-latch assertion, after the validator already returned
Lost. The later Ready-at-expiry row is not reached. Two earlier neutral rows
complete; these four internal rows are not four independent test executions.
The original 2 s audio deadline and real SCM_RIGHTS authority remain required.
New modeled V3 observations keep source freshness independent of that deadline.
The deadline already denies disclosure; no privacy revival is established.

The original native 90428 wrapper exits 1 because the later diagnostic admission
exits 125 before launching a body. That wrapper remains failed. Recovery
native 94823 exits 0 after retaining the actual failed driver, observing zero
backing inputs with both actual dsymutil and independent Mach-O inspection,
and verifying all **7,890 protected files**, unchanged source and owned-process
closure. The original driver remains independently retained in mode 0600.

Recovery runs only the previously unrun diagnostic: **1 pass / 0 failures**,
exit 0, retaining the original `<50 ms` assertion. For request 2, fixture work is
810 us, production Ahead readiness 664 us, production composition 685 us and
reported total 2,160 us; session-open counts remain 3 to 3. The phase print is
inside the outer clock, while the reported total is captured before that print.
This separate instrumented result does not explain the old full-run miss and
is not qualification. The intended RED control is not replayed in recovery.

Earlier native 81851 / exit 1 is a compile-input failure with zero test bodies; earlier
native 48603 / exit 1 reaches an unexpected camera-fixture failure, not the intended
latch assertion. Both remain distinct failures. See the
[preceding full-run receipt](../common-e927-host-failure-20261027/receipt.json)
for the separate complete local run; its five failures are not rewritten.

Corrective source `8cfcb464b`, with timing instrumentation excluded, has a fresh
112-control gate under native 36537 active at preparation. Formatting and
metadata exit 0; no final or runtime result is credited. No ARM64, Linux,
physical or readiness acceptance is claimed. Raw logs, words, private paths
and binaries stay outside the repository; the receipt binds six small hashes.
