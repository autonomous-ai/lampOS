# A body push reaches the head's contact recognizer

Part of #1798. Test-only follow-up on `9b6960e61`: no production source,
switch, topic, message field or product unit changes.

The existing tests checked the body message's freshness predicate, the
recognizer's direct push input and recorded-input replay separately. They
all passed when the runtime's call to `engine.set_head_pushed(pushed)` was
removed. The four new `touch_runtime::body_hand` cases use the real retained
bus message and Controls service with mock GPIO acquisitions, covering that
missing connection.

## Scenarios

- A fresh `body/hand` push suppresses Touch, Tap, Stroke, Hold and held time
  throughout a head contact longer than three seconds. The independent
  primary button still emits Stop, including its retained legacy request.
- An explicit false message or an expired true message clears the push but
  does not rescue that contact, even after another three seconds or a
  short release. A full release is required before a new tap or hold works.
  Both routes have separate tests. The expiry case retains the parseable
  true payload through recovery; no later false message can hide a broken
  expiry reader.
- A Touch already emitted before a push remains the same retained Stop
  request, with its original identity and timestamps. No later gesture is
  emitted by that spoiled contact.
- A push read at the same step as touch qualification prevents Touch. The
  case aligns the 620 ms acquisition with a due body-message poll, catching
  a read moved below GPIO processing.

Every step checks fresh, healthy GPIO acquisition, the actual head level,
arming after startup, and zero rejected inputs, reader errors or drain
overflows. Suppressed contacts keep their identity and no held time. These
checks prevent a stale or disabled fixture from passing by saying nothing.

## Clocks and boundaries

GPIO acquisitions and service steps advance together every 10 ms. Body's
`since_us` is deliberately from another monotonic origin. Bus headers keep
their real wall clock. The fixture's fresh publisher has no expiry, so a
slow host cannot accidentally expire it during the stepped scenario.
To test expiry, the fixture changes only the retained header's wall time
to the Unix epoch and its lifespan to 1,000 ms, then verifies that the bus
returns an expired, parseable true message from Body. It sleeps nowhere
and injects no clock into production code.

The release gap is measured from the last active acquisition: 7.19 s.
The old contact remains at 7.33 s (140 ms) and ends at 7.34 s (150 ms).
A new contact begins at 7.4 s, emits Touch at 7.42 s, is released at 7.6 s,
and emits Tap at 7.74 s. The next contact emits Hold exactly three seconds
after it begins. These are fixture acquisition-to-gesture boundaries,
not wall-clock throughput, physical latency, microphone readiness or sound.

This does not test Body's 80 ms publication cadence, its remaining
one-second TTL, the exact wall-clock expiry boundary, resisted-joint
detection or the physical pads. Body's deadband correction and the attended
tap/hold/sleep/wake/crash trial remain separate acceptance work. #2948
remains a draft until that trial qualifies it.

## Deliberately broken controls

Each temporary runtime change was applied alone, tested, and removed.
All failures below were executed assertion failures, not compile errors.
The runtime was restored byte-for-byte, with SHA-256
`c2a871c8e04fc6b2ee3302643092e983d9a081ed253a7363f8a5251a9d4d7d18`.

| Temporary fault | New tests | Existing predicate, recognizer and replay tests |
|---|---|---|
| Remove `engine.set_head_pushed(pushed)` | 0 passed, 4 failed | Each still passed, 1/0 |
| Move `look_at_hand(began)` below GPIO draining | 2 passed, 2 failed: ordering and retained Stop | Not rerun |
| Accept only true transitions | 2 passed, 2 failed: explicit-release and expiry recovery | Not rerun |

The focused command, from `lampOS/`, was:

```sh
CARGO_BUILD_JOBS=2 cargo test -q --offline --locked -p lamp-controls --test it body_hand:: -- --test-threads=1
```

Before mutation all four passed, none failed or were ignored. The three
older filters were `a_head_pushed_by_hand_asks_for_nothing`,
`the_bodys_hand_counts_only_fresh_and_from_the_body` and
`a_recorded_push_silences_the_head_in_replay`, using the same command with
each name in place of `body_hand::`.

## Gates

The unchanged Mac baseline passed 500 tests, zero failed, one ignored.
Its initial sandbox run denied Unix socket binds and failed 15 integration
cases; the unchanged run with local socket access passed all of them.
The final Mac source gate passed with Rust 1.96.0 and
`CARGO_BUILD_JOBS=2`, from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy -q --offline --locked --workspace --all-targets -- -D warnings
cargo test -q --offline --locked -p lamp-controls -- --test-threads=2
cargo build -q --offline --locked --workspace
```

The restored Controls suite passed **504 tests, zero failed, one ignored**:
33 library, two binary, 460 integration and nine strip-worker cases. The
native PR receipts and downstream gate are recorded in the pull request.
No local Linux ARM64 linking, device installation or lamp test is part of
this change.
