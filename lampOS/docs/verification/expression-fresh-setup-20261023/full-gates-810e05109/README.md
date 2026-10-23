# Expression fresh-setup native gate

One planned gate on base `810e05109bced3beca88a5b602930cfed5fba4e5` plus the exact two frozen Expression
test/docs snapshots completed with **7,164 passed,
1 failed and 32 ignored**. Native session
6502 exited **1**. No candidate commit existed at this
run; the base SHA alone is not the candidate identity. The two affected runtime
cases were **2 passed,
0 failed and
0 ignored**.

Every failure remains unwaived. This is not a causal baseline comparison,
Linux/ARM CI result or readiness decision. No test or full command was retried.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Expression, default parallel | 338 | 0 | 0 | 0 | 0 | 23.365243 |
| All 11 readers, default parallel | 6826 | 1 | 32 | 0 | 101 | 1548.025908 |

Both test phases compiled successfully. All actual Cargo launches, complete
terminal rows and separately listed compiled inventories bind with no unrun
or incomplete targets. Unsupported Update custom checks are ignored on Mac.
[census-and-outcomes.json](census-and-outcomes.json) contains every case outcome;
[affected-cases.json](affected-cases.json) binds the two changed runtime cases.
No historical case total constrains this current execution.

The retained failures are:

- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.

[failures.json](failures.json) preserves every complete diagnostic with its
byte offset, length and SHA256. Source identity alone is not a causal finding.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 12.56348 |
| clippy | 0 | 87.447927 |
| workspace-build | 0 | 57.621259 |
| diff-check | 0 | 0.050346 |

[command-results.json](command-results.json) records exact commands, bounds,
exits, elapsed times and output hashes. [gate-plan.json](gate-plan.json) is the
frozen plan. Tests used default parallelism, --no-fail-fast and RUST_TEST_THREADS
unset. Fully bound ordinary test failures remain failed while independent
planned gates continue. CPU/MAIN was released before packaging.

All 10,033 tracked paths, including
6,687 lampOS paths, docs, root lamp data fixtures, file
modes and literal symlink targets, were bound. The exact branch/base and the
two-file dirty state were required around commands. Input mtimes were refreshed
before target reuse without changing content. Initial and final aggregate:
`ae368921e44f27c88dbe750ac1f1926abd74c944727c359f8cfb270d7e53a05e`.
[source-and-environment.json](source-and-environment.json) records that proof.

[authored.patch.json](authored.patch.json) is the complete two-file patch; no
control hook was present. The authored patch and original assertion tails are
byte-identical to the six controls on 2615, whose qualification remains on that
historical base. [dependency-closure.json](dependency-closure.json) records
all manifests and normal/dev/build/platform edges for the 12-package closure.
[preparation-checks.json](preparation-checks.json) binds the reused census parser.

The authorized launch used require_escalated. Unix datagram and loopback TCP
bind/listen preflights passed before Cargo. Rust 1.96.0 used host
x86_64-apple-darwin; actual artifact file identities are retained. Physical
hardware ISA was not independently measured. Jobs 4, dev/test debug 0,
incremental 0, offline/locked, existing MAIN only. No ARM build or device access.

[native-streams.json](native-streams.json) includes complete exact stderr and
non-Cargo stdout. Cargo stdout is an explicit byte-range projection excluding
only parsed Cargo build records; every actual test row, summary and diagnostic
remains unchanged. Original and retained sizes, hashes and ranges are recorded;
the projection is not called unchanged full stdout.
[artifact-bindings.json](artifact-bindings.json) binds binaries before cache reuse.
[external-raw-index.json](external-raw-index.json) indexes full external raw
streams/inventories; local paths and mutable cache names are not remote
availability promises. All failures and retained test output/case rows are
published. Binaries, Python, complete Cargo JSON, source exports and bulk source
maps are excluded. [validation.json](validation.json) and
[manifest.json](manifest.json) record byte reconstruction, counts, links,
whitespace and payload identity. No new test runs occur during sealing.
