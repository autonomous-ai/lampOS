# Expression fresh-setup controls

All six planned observations matched on base
`2615c87b636d8e39d8df9548b00af7ed4e164f70` and its exact two-file candidate:
two old-fixture setup-expiry failures, two corrected-fixture setup-expiry
passes, and two corrected-fixture failures after actual input expiry.
Controller session 75812 exited 0 because these expected outcomes matched;
the four individual failing test runs remain failures with complete raw output.
No observation was retried. This is not the normal full package gate and does
not establish the cause of an earlier uninstrumented failure.

| Source | Control | Case | Test exit | Process wall seconds | Observed expiry age ms |
| --- | --- | --- | ---: | ---: | --- |
| before | setup | session/input loss | 101 | 0.636717 | 552 |
| before | setup | offline face | 101 | 0.630554 | 552 |
| after | setup | session/input loss | 0 | 0.693641 | 624 |
| after | setup | offline face | 0 | 1.240610 | 551, 551 |
| after | after_publish | session/input loss | 101 | 0.629220 | 551 |
| after | after_publish | offline face | 101 | 0.691361 | 551 |

These are first samples, including the 624 ms observed witness expiry. The
durations include the forced event wait and are not a speedup or device claim.
The unchanged lifespan is 500 ms and grace is 50 ms; expiry is strictly later
than their sum. The control waits for the actual retained-header expiry,
not a guessed completion sleep. The one-millisecond delay is a condition
polling interval. Exact header identities, payloads and wall samples are in
[observations.json](observations.json).

The setup witness uses `fixture/setup_expiry`, separate from every product
input, with the existing VoiceTiming declaration. Old actual VoiceTiming is
also verified expired before the first step. The corrected fixture publishes
its actual inputs only after setup and the witness event, and they are verified
fresh before stepping. The separate negative control waits for the actual
`voice/timing` header to expire; no actual input is republished or renewed.
The old and negative runs fail the original `fresh voice and joints` or
`a light request` expectations.

For the old offline case, false fails first and true is explicitly unrun.
The corrected setup control passes both original Listening/Unavailable
assertions. In the corrected post-publication control, false stays healthy
and completes its original assertion, then only true is expired. The private
Subscriber admission call is not instrumented: helper-side snapshots and the
actual original results are not relabeled as its private sampled clock.

All hooks are in the integration-test source. No product source, clock, TTL,
deadline or original assertion changed. The session test's complete input-loss,
stale-restart and new-session tail is byte-identical, as are both offline-face
assertions and populate's body. [candidate-review.json](candidate-review.json)
records the inverse-permutation proof; [candidate.diff.json](candidate.diff.json)
is the exact shipping diff at this control base. This receipt must not be
relabeled onto a later carry.

Both compiled inventories contain 27 cases and zero ignored cases; each actual
execution selected one case and filtered 26. See
[artifact-and-census.json](artifact-and-census.json). Compile commands passed,
and all six case rows, command arguments, exits and digests appear in
[command-results.json](command-results.json). The frozen plan is
[control-plan.json](control-plan.json), with its pre-hook freeze retained in
[plan-freeze.json](plan-freeze.json).

The actual Rust host is `x86_64-apple-darwin`, Rust 1.96.0; artifacts are Mach-O
x86_64. Physical hardware ISA was not independently measured. Local Unix
datagram and loopback TCP preflights passed before Cargo under the authorized
socket-enabled launch. Environment: offline/locked, jobs 4, dev/test debug 0,
incremental 0 and RUST_TEST_THREADS unset. No local ARM build or device access.

All 10,031 tracked paths, including docs and root data fixtures, were bound
with modes and literal symlink targets in each disposable source closure.
Both variants restored exactly in finally, and the shipping candidate's two
files/HEAD stayed unchanged. CPU/MAIN was released before sealing.
[source-and-restoration.json](source-and-restoration.json) binds these facts.

For replay, decode each JSON wrapper's `utf8` verbatim to recover the patch:
[before-control.patch.json](before-control.patch.json) applies to the base;
[after-control.patch.json](after-control.patch.json) applies after the authored
candidate diff. Build/invocation arguments and the mode/variant of each run
are recorded above. [hook-preparation.json](hook-preparation.json) records
successful patch checks and exact inverse-hook restoration. No product
instrumentation or Python orchestration is included in the published source.

[native-streams.json](native-streams.json) preserves every complete test stream
and all metadata/compile stderr as exact UTF-8 with original length and hash.
Only full Cargo JSON stdout is external, indexed in
[external-raw-index.json](external-raw-index.json). Local external paths and
mutable build-cache filenames are not remote availability promises.
Executables, Python scripts, bulk source maps and complete source exports are
excluded. [validation.json](validation.json) records byte/hash reconstruction,
JSON/link/whitespace checks; [manifest.json](manifest.json) binds every payload.
