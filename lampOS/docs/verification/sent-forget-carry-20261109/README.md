# Send receipt reconstructed after interrupted face forget

Status: reproduced failure, no production repair in this commit. This is an
integration HOLD, not a release or device qualification.

The tested source is parent `02fae6d762808ade6095dea29614fba2b874dbd6`
on train759 `caca127702`, plus the test in this commit. The parent composes
provider/playback cancellation (#5856/#5858), rename cleanup (#5792), face
outcome/context handling (#5874/#5878), the admission-time carried-round clear,
and the face speech extension. The new train contributes #5863's send records.

## Actual Engine sequence

1. The person says “Go ahead and send it, then forget my face.” The model calls
   `service_write`; its synthetic argument and result contain separate canaries.
2. A wordless line cuts that request while the send runner is held. The runner
   then returns `sent: true`; the resumed model receives that completed receipt.
3. The resumed model calls `forget_my_face` with `name: A`. The test waits for
   actual runner entry, then interrupts its pending deletion future with another
   wordless line. This is an admitted call, not an unasked model suggestion.
4. The next resumed model explicitly requests another `service_write`. The
   existing guard refuses it: the runner's write count stays exactly one.
5. The test inspects every model request after that second interruption. Both
   the earlier argument and result canaries have returned in raw tool messages.

The test fails on the argument-canary assertion after its one-write assertion
passes. The same resumed-request panic also contains the result canary. Result:
**0 passed, 1 failed; exit 101; 5.05 seconds**. The canaries are synthetic and are
not in the retained current human words. No real service, account or device is
used; services and deletion are controlled runner fixtures.

## Reproduction

Run from `lampOS/`, using Rust 1.96.0 and local socket permission:

```sh
TMPDIR=/tmp \
CARGO_TARGET_DIR=/tmp/fourth-face-carry-target \
CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_PROFILE_DEV_DEBUG=0 \
CARGO_INCREMENTAL=0 \
/Users/autonomous/.cargo/bin/cargo +1.96.0 test --offline -q \
  -p lamp-voice --lib \
  second_provider::tests::sent_write::a_send_then_cut_forget_then_cut_never_restores_payload_or_repeats_send \
  --jobs 2 -- --nocapture
```

Raw output is retained locally as `/tmp/fourth-face-carry-send-forget.log`.
An earlier compile was stopped with exit 130 to correct the fixture's missing
required face name; it is setup evidence only, not a product fail-first result.

Formatting and strict `cargo clippy -p lamp-voice --all-targets -- -D warnings`
passed for this test-only source. The intentional failing regression is not a
green product gate; no production repair or readiness is claimed.

## Repair boundary

Clearing carried rounds is insufficient: `SentWrite` retains the old call and
receipt, and `sent_with_words` reconstructs the round when it is absent. Dropping
that record outright would also remove protection against repeating the send.

An admitted forget needs sticky content redaction for the affected attempts,
while preserving content-free sent, uncertain and refused accounting. Recheck
that boundary after waits and before publishing carried content: already-cloned
calls, watch receivers and a late `services::send_state` result must not restore
payloads. A late result may update status only. Redacted notes must not tell the
model that discarded calls/results are available or invite reconstruction.

Before lifting the HOLD, cover late completion/fallback, unadmitted forget,
unknown-send accounting, multiple attempts, latest-origin selection and reused
model call IDs. This one failing Engine case does not establish those controls,
whole-suite success, real-model behavior or the separate acoustic stop target.
