# Touch readers tolerate added metadata

The one-build-apart rule requires an older reader to ignore a newer
publisher's additional fields. The three touch records previously refused
every unknown field. This change removes only that blanket refusal from
`TouchEvent`, `TouchSourceState` and `TouchState`. It publishes no new field,
changes no topic or version, and keeps existing serialized output unchanged.
Physical privacy evidence retains its separate strict contract.

This is a reader-first prerequisite for a future additive publisher, not
evidence that a currently released adjacent pair fails. Deployed older
binaries remain strict; they cannot be made tolerant retroactively. A new
action or enum meaning is not merely an additional field.

## Tests first

The baseline is `b3a1ece0b`. A scratch probe linked against Cargo's exact
emitted Msgs library accepted original event and state literals, including
their semantic checks. Adding a `future_diagnostic` field failed at each of
four independent placements: a direct event, an outer state, a source in
that state, and its retained event. The existing Body `Hand` record already
accepted added fields and refused a missing required `since_us`; it needed
no change.

The seven focused regression cases were added before the production edit.
The following command, from `lampOS/`, initially produced **one passed and
six failed** among those cases. Every failure was an executed unknown-field
decode rejection. The negative known-field case already passed. Removing
the three annotations changed the result to **seven passed, zero failed**:

```sh
CARGO_BUILD_JOBS=2 cargo test -q --offline --locked --no-fail-fast -p lamp-msgs -p lamp-controls additive_fields -- --test-threads=2
```

Three existing tests also match that filter: speaker state, rest requests
and scheduled due messages. They passed before and after, making the whole
filtered command **4/6 before and 10/0 after**. One existing touch test was
expanded and renamed; the net addition is six tests.

- Msgs reads a frozen original JSON literal independently of the current
  serializer. New metadata at each level preserves the known value,
  Stop/Notice request semantics and serialized bytes. An extra `permit`
  field is discarded, not interpreted or written back.
- Required fields and types, unknown source/gesture/intent values and an
  invalid combination of known gesture and intent remain refused.
- Controls delivers direct and retained events once. Repeated event/state
  inputs and a later heartbeat cannot duplicate the gesture.
- Extra metadata cannot authorize disabled, stale, future-clock or
  foreign-clock input, the wrong owner/reader, or a retired owner/reader.
  The event cases keep their surrounding state fresh and accept the next
  valid event with the same identity, so a broken fixture or an incorrectly
  consumed identity cannot manufacture the negative result.

No receiver algorithm, privacy authority, cancellation path, person data,
switch, unit or census is changed. This is a default parser change, not a
switched feature. The shared Msgs dependency is checked across the workspace.

## Host parsing latency

The target set before the edit was p99 below **1 ms** for the existing
payload's decode and semantic check on the Mac. No I/O, wait, polling or
queue was added. The same 421-byte event literal was parsed 100 times to
warm up, then 10,000 times with `Instant` around
`serde_json::from_str::<TouchEvent>`, `TouchEvent::check()` and consumption
through `black_box`. Allocation and value destruction are inside the timed
boundary. Output happens after all samples have been taken.

The scratch caller was compiled with `rustc -O` and linked to Cargo's exact
emitted **debug** libraries, once before and once after the three annotation
removals. This is a repeated host parser microbenchmark, not cold startup,
ARM64 throughput, physical touch latency or acoustic readiness.

| Mac host, nanoseconds | Before | After |
|---|---:|---:|
| Median | 7,451 | 7,422 |
| p95 | 7,748 | 7,745 |
| p99 | 8,331 | 8,485 |
| Maximum | 42,805 | 68,320 |

Both meet the host target. The small differences do not establish a speed
improvement. First-use and end-to-end behavior on a lamp were not measured.
The attended head-touch acceptance, including restart followed by a fresh
tap, remains separate; #2948 remains a draft for that trial.

## Gates

Rust 1.96.0 on the Mac, with `CARGO_BUILD_JOBS=2`, from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy -q --offline --locked --workspace --all-targets -- -D warnings
cargo test -q --offline --locked --workspace --no-fail-fast -- --test-threads=2
cargo build -q --offline --locked --workspace
```

On the authored diff over `b3a1ece0b`, formatting and strict workspace
Clippy passed. The full workspace tests passed **10,777 tests, zero failed,
65 ignored**, exit zero, in 3,722.5 seconds. This total includes the separate
12-case custom harness. The full workspace build passed, exit zero, in
73.5 seconds. The PR distinguishes this full baseline run from subsequent
checks after carrying the unchanged authored patch onto a newer base.

The unchanged Msgs baseline passed **171 tests, zero failed, zero ignored**
with local Unix socket access. Its initial sandbox attempt passed 169 and
failed one socket bind with `PermissionDenied`; the doc test did not run
after that failure. No source changed between attempts.

The PR records native Linux ARM64 build/tests, Linux Clippy and the explicit
Linux formatting receipt on the pushed source. No local ARM64 linking,
device installation, lamp access or physical measurement qualifies this
change.
