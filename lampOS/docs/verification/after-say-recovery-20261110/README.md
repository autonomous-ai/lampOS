# After-say replay completion

Third's carry of Fifth's #5902 is `f667b5b43` on base `75da83b34`;
Fifth's original head was `f8ed71dc115828f1d1ad188951bae8f2c6978cb5`.
These results cover the local uncommitted repair and regression tests on
that carried head.

## Behavior and scope

After a wordless cut, the existing six-second recovery window may admit
one replay. Its after-say actions now require a fresh explicit `Resting`
state and a successful final drain of heard messages. Missing, stale or
malformed state, unreadable heard data, another cut, Thinking at the
four-second bound, or the 180-second speech deadline cancel those actions.
Initial-line and search behavior stay unchanged. There is no new retry,
thread, network call or additional wait; the existing poll is 20 ms.

## Retained process-and-bus evidence

Six separate regressions exercise a resumed heartbeat becoming stale after
3.2 seconds of real time, malformed voice state, Thinking beyond four
seconds, Speaking beyond 180 seconds using the fixture clock, malformed
Speech during replay, and malformed Speech immediately before Resting.
Each requires no after-say action and a cancellation reason.

- [Fail-first log](fail-first.txt): 0 passed, six failed, 16.25 s, with the
  new tests against the carried production behavior.
- [Repaired log](after.txt): six passed, zero failed, 10.01 s.
- [Positive replay control](positive.txt): one passed, 2.60 s; a wordless
  cut followed by replay and Resting still performs its after-say show.

The first two invocations select the `a_resumed_say_` integration tests in
`lamp-skills --test it`; the positive control is
`a_say_cut_by_a_sound_with_no_words_is_said_again_and_its_after_steps_are_made`.
The complete local gate and host timing diagnostic are recorded below.

## Timing and observation limits

Host target: under 250 ms from invalid-state publication to observed
cancellation, first and repeated calls. The real three-second freshness
window is separate from reaction time. The diagnostic below measures this
limited process-and-bus boundary.

Malformed Speech tests a payload parse failure, not an injected raw
Subscriber I/O error. The bus itself skips malformed wire envelopes,
expired messages and wrong types before the caller receives them; an empty
drain therefore cannot prove that every interruption was delivered. The
Resting-boundary test publishes malformed Speech immediately before
Resting, but scheduling may let the first drain observe it. It is not a
deterministic final-drain-only test. Separate state and heard topics also
cannot prevent words arriving after the final drain from missing that
completion decision.

These authored process-and-bus events do not establish acoustic completion,
motor latency or device behavior. No device action was performed.

## Host cancellation latency

`cargo test -q -p lamp-skills --test it resumed_say_unknown_state_first_and_repeated_cancellation_latency -- --ignored --nocapture`
passed (one test, 2.04 s). The same local process handled first and repeated
calls. From immediately before publishing malformed state to observing the
run's Done bus event: **21,638 us first / 14,232 us repeated**, both under
250 ms. No after-action colours were emitted. See [raw timing](latency.txt).

This boundary includes publication, process scheduling, the existing 20 ms
poll and receipt of Done. It excludes fixture initialization, speech,
heartbeat expiry and device output. The fixture does not advance its fake
clock or insert a fixed sleep inside the measured interval. This is a
functional cancellation repair, not a before/after latency improvement.

## Source and local commands

- `crates/skills/src/main.rs` SHA-256: `9e208aef444e225a9bf40ba003becd4d93b22563507c11144aaf3299962834a2`.
- `crates/skills/tests/it/skills.rs` SHA-256: `347e3cc1ae5e4f9f681ac7ab161e43f6aa1fca6c44f5ea5de356762a9bbf5d62`.

All Rust checks run from `lampOS/` with Cargo jobs 2 and the shared target
directory, using the pinned toolchain. Final gate commands are
`cargo fmt --all --check`,
`cargo clippy -q -p lamp-skills --all-targets -- -D warnings`, and
`cargo test -q -p lamp-skills -- --test-threads=1`. The tests use local
processes/sockets. Formatting, strict lint and the full test command completed successfully
(exit 0). The [full suite](full-suite.txt) passed 675 tests, with 10 ignored:
191 library, 23 main, 428 integration (two ignored), two release-pair
(eight ignored), and 31 requested-audio cases. Integration tests took
295.77 s. The ignored host timing diagnostic was run separately above.
These local results do not substitute for Linux/ARM CI or the played check.

Maintained-document links: 192 docs, 1900 links, zero broken. The checker
tests passed all nine cases. Historical verification directories are
excluded from the outgoing-link scan.

## Equal rebase onto the current integration base

Both commits compare equal after rebase onto `c29008d21`, runtime head
`ebea47ed7d0847713856118f39d17d1ce4d3ced5`. Skills, messages, bus and Cargo
manifest/lock files are unchanged by the base advance; Voice changes are
in person naming/worker prompt context, outside this Skills recovery path.
The 675-test full gate above remains explicitly on the prior base.

Current-base `cargo fmt --all --check` and
`cargo clippy -q -p lamp-skills --all-targets -- -D warnings` passed.
Actual integration controls, all serial except the single exact test:

- `cargo test -q -p lamp-skills --test it cut -- --test-threads=1 --nocapture`: [four passed](current-cut.txt), 6.69 s, including show/dance replay and real-word/no-resume cancellation.
- `cargo test -q -p lamp-skills --test it resumed_say -- --include-ignored --test-threads=1 --nocapture`: [seven passed](current-resumed.txt), 10.89 s, six regressions plus the explicit host timing diagnostic.
- Exact `skills::words_heard_after_the_line_is_said_again_still_cancel`: [one passed](current-late-words.txt), 1.57 s.

Current first/repeated invalid-state publication to observed cancellation:
**18,020 us / 10,475 us**, same boundary and under the same 250 ms target.
This is host evidence only. [Current docs](current-docs.txt): 192 documents,
1900 links, zero broken. The evidence commit adds no runtime change.
Fresh ARM/Linux CI and the two reads still qualify the published carry.
