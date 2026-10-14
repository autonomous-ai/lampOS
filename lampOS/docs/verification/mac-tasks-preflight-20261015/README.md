# Mac Tasks fixture times out before the descendant case

Ready for lamp-voice: no

During the serial full workspace gate at Memory head
`5fa0f78d67442107bd5b9c000a9ff2e57d7c67d1`, based on
`dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`, the Tasks `harness` target
finished **23 passed / 1 failed**, in 6.71 seconds. The failing test is
`a_descendant_holding_pipes_is_killed_without_an_unbounded_drain`,
`crates/tasks/tests/harness.rs:509`:

```text
left: NotSent { code: "preflight_timeout" }
right: Uncertain { code: "pipes_open" }
```

This was x86_64 macOS 26.6.2, Intel i7-10700K, 64 GiB, Rust 1.96.0.
Four build jobs, incremental compilation disabled, one test thread, with
no competing local build or test started by Fourth. It uses the repository's
scripted executable, not installed Harness, a provider, an account, a
remote agent or a lamp. The full gate is still running; no final exit
status or complete workspace count is claimed yet. `failure-excerpt.txt`
is explicitly a partial excerpt, with its original whitespace retained.

## Found, not mine

All three relevant Tasks files match upstream byte for byte; hashes are
in `source-sha256.json`. `Rig::new` sets its read and launch timeouts to
one second (`tests/harness.rs:34-38`). `Cli::launch` first runs the
`machines --json` preflight using the read timeout; its timeout returns
`NotSent { preflight_timeout }` before it sends `new`. The fixture's
`machines` response has no injected delay. The test was first in this
target's execution order; all 23 later tests passed. Production defaults
are ten seconds for reads and sixty seconds for launch.

The failed assertion therefore does not demonstrate a descendant cleanup
fault: `new` and the following heartbeat/reaping assertions were not
reached. Source already notes possible first-execution checks on newly
copied binaries in the separate identity fixtures, but no process-stage
timing was captured here. Cold executable startup or scheduling are
possible explanations, not measured causes. Do not lengthen production
deadlines or remove cleanup assertions on this evidence.

Fourth has not changed Tasks. After the current full gate ends, the
same compiled fixture will be checked in isolation, with the original
failure retained. Its owner can then assess whether the fixture needs
separate preflight/startup evidence before timing descendant cleanup.
