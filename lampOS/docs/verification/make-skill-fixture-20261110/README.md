# Make-skill chat fixture isolation

Fourth claimed the narrow fixture investigation on Tests #1569 in comment
6754069755. It is separate from Second's chat_second_try repair. No current
owner of this helper was found in that issue's comments; unpublished work
was not ruled out. Base is a2260fd1b85ed901ced9d1a083c893874b1d6bd3.

## Reproduction

The actual make_skill_local::tests::service helper accepted a complete
GET /v1/models as its next chat request and panicked at its required-brief
body assertion. The subsequent real local Chat/make call could not obtain
the scripted reply. The added discovery regression failed on the original
helper: zero passed, one failed, zero ignored, 0.01 s body, exit 101.
This is an injected-request reproduction, not attribution of the earlier
uninstrumented full-suite failure or identification of a background process.

## Test-only change

Only the cfg(test) module changes. The complete production prefix is
unchanged, SHA-256 d3e81208c87d0310a86ae28bd81bf74e4950a2abdf425f0429e245c6d0f9f042.
The shared service and chat_at interfaces used by app_make stay unchanged.
An exact POST /v1/chat/completions request with this fixture's token is
required before consuming a reply. Header names compare without case;
tokens compare exactly. The token is derived from the fixture URL and
distinguishes concurrent ports; it is neither a security credential nor
unique across later reuse of a port.

The service uses a current-thread Tokio runtime inside its existing server
thread. At most sixteen initial readers are retained. A reader has one
absolute two-second deadline for its complete request, capped at 64 KiB.
Each next authenticated chat has a ten-second overall deadline that foreign
traffic cannot reset; response writes have two seconds. These are test
fixture limits, not production response-time guarantees. Idle/partial
foreign readers cannot serialize the ordinary two-reader control case.
At full reader capacity, accepting waits for a reader to finish; this is
not immunity to arbitrary local traffic.

Authenticated malformed framing, incomplete bodies, missing required brief
content and unexpected tools fail visibly. Genuine duplicate requests still
consume separate entries in the scripted plan. Body checks retain the existing
brief/tools substring assertions; they are not general JSON validation.
Script exhaustion keeps the
existing lifetime boundary; this does not detect every later extra request.
No production retry, model, memory, device or interactive UX behavior changes.

## Validation

Fail-first command from lampOS, Rust 1.96.0, short owned TMPDIR, sole retained
Cargo target, DEBUG0, incremental off, two build jobs:

```sh
cargo +1.96.0 test --offline -q -p lamp-voice --lib make_skill_local::tests::discovery_does_not_consume_the_skill_chat_reply -- --exact --nocapture --test-threads=1
```

The repair gate runs these commands sequentially from lampOS and stops on
its first failed stage:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -q -p lamp-voice --lib make_skill_local:: -- --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --lib app_make:: -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2
cargo +1.96.0 build --offline -q --workspace
```

After the full-suite failure below, the separate remaining-target controller
uses the same environment and frozen source. Each named failing case is run
once with `cargo +1.96.0 test --offline -q -p lamp-voice --lib <fully-qualified-case> -- --exact --nocapture --test-threads=1`, followed by:

```sh
cargo +1.96.0 test --offline -q -p lamp-voice --test '*' -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --bins --examples -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --doc
cargo +1.96.0 build --offline -q --workspace
```

The first repair gate passed whole-workspace formatting (44.631 s), then
stopped during strict Clippy compilation (581.610 s, exit 101): E0282 for
an untyped JoinSet, with a cascading unsized-body E0277. No repair tests or
workspace build ran. The explicit FixtureRead result alias and typed JoinSet
correct that inference boundary. The
initial failure logs and receipt are retained separately as attempt1.

Corrected-source whole-workspace formatting passed (45.485 s), as did strict
workspace all-target Clippy (314.630 s). The guarded Rust file SHA-256 is
d648d6a280e5833b82290c4521c9d42bfd3cee6b4c1fc0539de0cc3db8358226,
unchanged through both stages and the focused run. Focused tests then stopped
with 25 passed, one failed, zero ignored (11.19 s body, 202.218 s command).
The new discovery test successfully served exactly one genuine chat, but
asserted the response key name rather than the actual successful key made.
This assertion is corrected; attempt2 logs and receipt are retained. Shared
app tests, full Voice and workspace build did not run in that attempt.

Attempt3 on the corrected assertion passed whole-workspace formatting
(47.323 s) and strict all-target workspace Clippy (308.279 s). Source SHA-256
77757d8a713ef03a6433c7a66be9dfa3f7ab553ececd25833cfe058b46c6fb6b
remained unchanged. Focused tests passed: 26 passed, zero failed or ignored,
20.94 s body. The two authenticated exchanges with idle/partial probes
completed in 672.659 us in that host control; this is one fixture sample,
not a production/device latency claim. Expected panic hooks from negative controls did not fail the
suite. Shared-app tests passed: 14 passed, zero failed or ignored,
71.96 s body (75.417 s command).

Full Voice then failed: 5,460 passed, four failed, sixteen ignored,
1,978.34 s body (2,215.729 s command, exit 101). Source hash stayed unchanged.
The four failures are in unchanged coordinator tests:

- an_exact_line_is_shown_speaking_with_its_own_level: StaleOwner from the
  resting-owner helper at coordinator/tests/say.rs:837.
- the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken:
  scenario host watchdog and fixture-child exit-observation timeout.
- the_name_wakes_a_sleep_and_its_capture_is_closed_before_the_wait_opens:
  capture message age 135,838 us exceeded the 100,000 us bound, then no start.
- sigint_retains_failure_and_acknowledged_synthetic_diagnostics:
  signal child did not finish within its four-second deadline.

No root cause is attributed from these symptoms. The failed full run is
retained and will not be rerun for a green result. Its controller and the
specifically reported child PID were no longer present on targeted inspection.
A separate controller ran each exact failing case once as a diagnostic:
all four passed, one case each, zero failed or ignored. Body durations in the
order above were 0.50 s, 13.80 s, 1.01 s and 2.50 s. These passes do not
replace the failed full-suite result. The integration stage passed 279 tests, zero failed, six ignored.
Binary/example tests passed eleven cases, zero failed or ignored. The Voice
doc command passed with zero cases. Workspace build passed in 316.324 s.
Every remaining stage retained the final Rust source hash above.
Documentation links pass: 192 documents, 1,898 links, zero broken.
No current-head Linux CI or readiness claim.
Logs are retained outside the repository under /tmp/fourth-make-skill-fixture-*.

## Runtime-base revalidation, 10 November

The unchanged patch was rebased onto a9eb18ecea as local head
1b27714c9c0f81d0b0b5e3ffb872d3e8ca2d670a. The helper hash remains
77757d8a713ef03a6433c7a66be9dfa3f7ab553ececd25833cfe058b46c6fb6b.
This new runtime base justified a fresh gate; it was not a retry of the
prior full-suite result. Formatting passed in 29.662 s, strict workspace
Clippy in 351.181 s, 26 fixture cases in 31.02 s body (434.210 s command),
and 14 shared-app cases in 169.89 s body (174.935 s command).
Two authenticated chats alongside idle/partial probes took 607.911 us in
one host sample; no production or device timing is inferred.

Full Voice failed: 5,468 passed, five failed, sixteen ignored,
1,354.18 s body (1,469.675 s command). Failures, all in unchanged files:

- coordinator::tests::sleep_name::the_name_wakes_a_sleep_and_its_capture_is_closed_before_the_wait_opens:
  capture message age 102,173 us exceeded the 100,000 us bound.
- service_privacy_controls::tests::a_started_ring_that_failed_still_needs_its_black_write_and_exit:
  privacy freshness lost while waiting.
- sleep_capture::tests::another_reader_stops_the_sleep_capture_and_the_sleep_stays_full:
  sleep control connection reset.
- sleep_capture::tests::nothing_heard_asleep_reaches_the_first_turn_after_the_wake:
  sleep grant connection reset.
- sleep_capture::tests::two_hearings_wake_it_closed_first_and_shadow_never_wakes:
  sleep grant connection reset, then the expected hearing state was absent.

One exact diagnostic per failed identity used the same unchanged source.
Another-reader and nothing-heard-after-wake passed; the other three failed
again. Sleep-name age was 192,956 us; privacy cleanup also reported an
invalid/duplicate shutdown receipt; two-hearings again lost the sleep grant.
No whole-suite or diagnostic retry was performed to obtain green output.

The separate integration `it` target failed: 276 passed, one failed,
79.25 s body. All twenty light calls returned Done, but their measured
median was 105.940808 ms against the test's less-than-25 ms target.
This unchanged latency test was not retried. The remaining integration
targets passed two cases with six ignored; binaries/examples passed eleven
cases; the Voice doctest command passed with zero cases.

A temporary SDK dependency installer elsewhere unexpectedly began a Rust
source build during the full run. Its identified process group was stopped
and verified gone; later SDK checks used binary wheels only. This host-load
incident is disclosed without assigning it as any failure's cause. Exact
diagnostics ran after it had stopped. No test timeout or unrelated source
was changed. Failure reports are on #5932 (comment6756925504) and Tests
#1569 (comment6756926027). Logs and stage receipts remain under
/tmp/fourth-make-skill-fixture-a9eb-*.

Workspace build passed in 179.269 s. The later rebase onto
4d86f6f0dd changes only joining.md above a9eb; range-diff is equal and the
validated Rust helper hash remains unchanged. Fresh CI is pending.
