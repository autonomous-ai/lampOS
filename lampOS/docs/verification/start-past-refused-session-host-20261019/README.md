# #1567: a session refused at start, on the host (2026-10-19)

The live case: lamp-75c9, 19 Oct 16:19 +07, a 16 Oct lamp-voice build. The
model account was out of quota; the provider worker said `unavailable`
(`quota_exceeded`) 1 s after `session_boot`, and 25 s later the voice exited
with "voice input never became ready", ten times, then `failed`.

Developed on a Mac (Darwin 25.6, Rust 1.96.0) at `origin/lamp-voice`
`f9c9f8d9b`. No lamp, network service or model was used: the coordinator's
peers are the stepped fixtures of `coordinator/tests/endpoint_session.rs`,
the provider link is the fake service of `lamp-gemini`'s tests.

## What it shows

- The serving link (`RecoveryPolicy::endless`, `serving_recovery_policy`)
  turns a quota refusal of its first connection into one `Unavailable`
  update and asks again only after `refusal_pause` (120 s in the product,
  1 s in the test), once; the session that comes up is reported fresh.
  It never becomes `Terminal` for a quota or rate refusal on that link.
- The coordinator, with `LAMP_START_OFFLINE=on`, a `Serve` lifetime and the
  microphone retained, treats that refused session as a link not up yet: it
  stays past the 25 s budget, records `waiting_for_link` once and
  `provider_link_current` once when the later session comes, and runs on to
  the mute that ends the cycle. Quota and rate refusals alike.
- With the switch off, the same run ends at 25 s with "voice input never
  became ready": the lamp-75c9 symptom. The product unit carries
  `LAMP_START_OFFLINE=on` only since e0e23ef8c (17 Oct 16:09 +07); the unit at
  the last commit of 16 Oct does not. The cause on lamp-75c9 is the old
  unit/build, not the current start-up path.
- Slow ears: a microphone that starts 20 s after it is asked is waited for
  (the run lives past the budget to its link); one that starts 26 s after
  ends the voice at the budget even with the switch on, before any link.

## Commands

Environment: `RUSTC_WRAPPER=` `CARGO_PROFILE_DEV_DEBUG=0`
`CARGO_PROFILE_TEST_DEBUG=0` `CARGO_INCREMENTAL=0`, `-j2`, from `lampOS/`.

| Gate | Command | Result |
|------|---------|--------|
| fmt | `cargo +1.96.0 fmt --all --check` | clean |
| clippy | `cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings` | clean |
| focused, voice | `cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 started_offline a_session_refused_at_start slow_ears_within a_service_started_with_no_link` | 5 passed |
| focused, link | `cargo +1.96.0 test -j2 -p lamp-gemini --lib -- --test-threads=1 endless` | 10 passed |
| full voice | `cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1` | 2,327 passed, 0 failed, 12 ignored (32 binaries; lib 2,087) |
| build | `cargo +1.96.0 build -j2 --workspace` | ok |

New tests:
`coordinator::tests::endpoint_session::a_session_refused_at_start_does_not_end_a_service_started_offline`,
`coordinator::tests::endpoint_session::slow_ears_within_the_budget_are_waited_for_and_past_it_end_the_voice`,
`recovery::tests::endless::a_first_connection_refused_for_quota_is_asked_again_after_the_pause`
(`lamp-gemini`).

## Not shown

On a lamp: a unit with the switch and a spent quota, left past 120 s to see
the second ask in the journal.
