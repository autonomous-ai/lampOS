# Sign-in code delivery overlap, 23 October 2026

Ready for lamp-voice: no. Part of #2839. A product correction needs approval;
this receipt changes only tests and documentation. No lamp or played script
was used. The original lamp report's unique cause is not established.

Base: `b7700b7c268c0c22e26c1c88c9e9ee359cddf550`.
Both executions used the same command from `lampOS/`, Rust 1.96.0
`x86_64-apple-darwin`, offline, four build jobs, debug information and
incremental compilation disabled, with a serial two-test filter:

```sh
cargo test -p lamp-voice --lib --offline sign_in::tests::code_delivery:: -- --test-threads=1 --nocapture
```

| Receipt | Passed / failed / ignored | Cargo exit | Observation |
|---|---|---|---|
| `first-invalid-subscriber/` | 0 / 2 / 0 | 101 | Fixture subscriber `code-test` violated the bus name grammar; neither test started Tasks or reached the product scenario. |
| `corrected-overlap/` | 1 / 1 / 0 | 101 | Direct code delivery and one requested repeat passed. Queued news overlapping START delivered the code twice before a repeat request: actual 2, expected 1. |

Both compiled successfully and filtered out 3,562 tests. The corrected two
cases took 0.86 seconds; this is not an acoustic or performance measurement.
The sole source correction between executions was `code-test` → `code_test`.
Each frozen source was executed once; no failed product assertion was retried.

The corrected failure is `sign_in.rs:1360` in
`sign_in::tests::code_delivery::a_queued_sign_in_code_is_not_repeated_by_the_pending_start`.
The explicit-repeat payload, unchanged-code news suppression, and one-Harness-
session assertions ran before the failing count and passed. The aggregate
assertion after that failure did not execute.

The real Tasks service uses an event-controlled stand-in Harness adapter.
A relay holds only the first `starting` response until the code read from
Tasks is in the actual TaskNews queue; it then lets START continue. The
count covers tool-result and queued-news delivery payloads. No new completion
sleep or production deadline change was used. Original product source and
all existing tests are byte-identical after removing the new cfg(test) module.

Source SHA-256:

- First `voice/src/sign_in.rs`: `95324c355544d1b71e93900a5e3a4bbaf66f85386c23bfc732564f36305fc3ff`.
- Corrected: `3107b74fe2e4223bbe9f8df22237f42b350513faa94675492af5e3326fdde50f`.
- `docs/tasks.md`, both: `d5a42e81fd8de2c6434f1df9123b66d6e12ee9cafb959e5d74ad333a1eb10035`.

Preserved test artifacts, both x86_64 Mach-O, 114,668,632 bytes:

- First: `46ff72213c46b64bab98f639d3d5ba67fd496fed94f3cb70554c81a77c75dae3`.
- Corrected: `03bbf8c230f5d64bf172448dd50bac71a6d96195ebef0d37d1fa6a1d7cf54823`.

The binaries remain in the external run directories named in metadata; they
are not included here or offered as Linux/ARM artifacts. SMALL was released
at terminal and later its build cache could be removed without losing them.
MAIN was never touched by these runs.

`source-change.patch.json` contains the patch that replays the corrected test/docs change from the base;
`source-before-fixture-correction.patch.json` contains the first version.
`fixture-identifier.patch.json` contains their exact one-line difference.
`source-closure.json` and both before/after maps prove all 6,526 tracked lampOS
file bytes, including documentation, stayed equal during each execution;
only the subscriber-name line differs between executions.

Cargo stdout/stderr, patches, and recorded toolchain, hardware and Git-status
outputs are complete, lossless UTF-8 JSON wrappers. The saved artifacts' file
and lipo outputs were collected read-only while packaging, without executing
the binaries again. Each wrapper records the original
stream's byte length and SHA-256; encode its `text` as UTF-8 to reconstruct
those bytes. For a patch, for example, `jq -j .text source-change.patch.json`
writes its exact original bytes; apply it only to the named base. `SHA256SUMS` hashes the published files themselves, not the
unwrapped streams. No binary, Python runner or complete Cargo JSON is included.
The source identities, exact environment and terminal observations are in the
JSON receipts. The following product recommendation is a read-only proposal:
[correction-scope.md](correction-scope.md).
